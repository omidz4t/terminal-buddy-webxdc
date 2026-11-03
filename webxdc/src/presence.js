/**
 * Presence + peer cursors + live reactions.
 * Patterns from context/editor (collab/presence.ts, ReactionPie.vue).
 *
 * Peers publish JSON on the shared realtime channel (first byte `{`):
 *   join | leave | pos | writer | in | rx
 * These fan out **naturally** to other webxdc clients via Iroh gossip.
 * The agent host may observe them for its TUI but does not rebroadcast.
 *
 * Terminal binary frames (I / O / R / P / E) stay separate; only the host
 * produces O/E.
 */

const USER_COLORS = [
  { color: "#30bced", light: "#30bced33" },
  { color: "#6eeb83", light: "#6eeb8333" },
  { color: "#ffbc42", light: "#ffbc4233" },
  { color: "#ecd444", light: "#ecd44433" },
  { color: "#ee6352", light: "#ee635233" },
  { color: "#9ac2c9", light: "#9ac2c933" },
  { color: "#8acb88", light: "#8acb8833" },
  { color: "#1be7ff", light: "#1be7ff33" },
];

export function pickUserColor(addr) {
  let hash = 0;
  for (let i = 0; i < addr.length; i++) {
    hash = (hash << 5) - hash + addr.charCodeAt(i);
    hash |= 0;
  }
  return USER_COLORS[Math.abs(hash) % USER_COLORS.length];
}

const POINTER_SEND_MS = 16;
const STALE_MS = 12_000;
const POINTER_STALE_MS = 30_000;
const WRITER_STALE_MS = 4_000;

export class Presence {
  /**
   * @param {string} selfAddr
   * @param {string} selfName
   * @param {(data: Uint8Array) => void} send
   */
  constructor(selfAddr, selfName, send) {
    this.selfAddr = selfAddr || "local";
    this.selfName = selfName || selfAddr || "me";
    this.send = send;
    this.colors = pickUserColor(this.selfAddr);
    /** @type {Map<string, any>} */
    this.peers = new Map();
    this.listeners = new Set();
    /** @type {Set<(r: any) => void>} */
    this.reactionListeners = new Set();
    /** @type {Map<string, number>} recent reaction ids → expiry (de-dupe host relay) */
    this.seenRx = new Map();
    this.pendingPos = null;
    this.lastPosKey = "";
    this.reactionSeq = 0;
    this.pointerTimer = setInterval(() => this.flushPointer(), POINTER_SEND_MS);
    this.staleTimer = setInterval(() => this.gc(), 1000);
    this.announceJoin();
    this.joinTimer = setInterval(() => this.announceJoin(), 5000);
  }

  destroy() {
    clearInterval(this.joinTimer);
    clearInterval(this.pointerTimer);
    clearInterval(this.staleTimer);
    this.sendJson({
      t: "leave",
      a: this.selfAddr,
      n: this.selfName,
      c: this.colors.color,
      cl: this.colors.light,
    });
  }

  onChange(handler) {
    this.listeners.add(handler);
    handler(this.list());
    return () => this.listeners.delete(handler);
  }

  onReaction(handler) {
    this.reactionListeners.add(handler);
    return () => this.reactionListeners.delete(handler);
  }

  list() {
    return [...this.peers.values()].sort((a, b) =>
      (a.name || a.addr).localeCompare(b.name || b.addr),
    );
  }

  emit() {
    const list = this.list();
    for (const h of this.listeners) h(list);
  }

  emitReaction(reaction) {
    for (const h of this.reactionListeners) h(reaction);
  }

  sendJson(obj) {
    const bytes = new TextEncoder().encode(JSON.stringify(obj));
    this.send(bytes);
  }

  announceJoin() {
    const now = Date.now();
    if (this.lastJoinAt !== undefined && now - this.lastJoinAt < 1000) return;
    this.lastJoinAt = now;
    this.sendJson({
      t: "join",
      a: this.selfAddr,
      n: this.selfName,
      c: this.colors.color,
      cl: this.colors.light,
    });
  }

  announceWriter() {
    this.sendJson({
      t: "writer",
      a: this.selfAddr,
      n: this.selfName,
      c: this.colors.color,
      cl: this.colors.light,
      at: Date.now(),
    });
  }

  /**
   * Always-on shared cursor (stage-local coords).
   * @param {number} x
   * @param {number} y
   * @param {'down'|'up'} button
   * @param {{ immediate?: boolean }} [opts]
   */
  updatePointer(x, y, button = "up", opts = {}) {
    if (!Number.isFinite(x) || !Number.isFinite(y)) return;
    this.pendingPos = {
      a: this.selfAddr,
      n: this.selfName,
      c: this.colors.color,
      cl: this.colors.light,
      x,
      y,
      b: button,
    };
    if (opts.immediate) this.flushPointer(true);
  }

  flushPointer(force = false) {
    if (!this.pendingPos) return;
    const p = this.pendingPos;
    const key = `${p.x | 0},${p.y | 0},${p.b}`;
    if (!force && key === this.lastPosKey) return;
    this.lastPosKey = key;
    this.sendJson({ t: "pos", ...p });
  }

  /**
   * Ephemeral reaction burst at stage-local coords (editor-style `rx`).
   * @param {string} emoji
   * @param {number} x
   * @param {number} y
   */
  sendReaction(emoji, x, y) {
    const e = (emoji || "").trim().slice(0, 16);
    if (!e || !Number.isFinite(x) || !Number.isFinite(y)) return;
    this.reactionSeq += 1;
    const id = `${this.selfAddr}-${Date.now()}-${this.reactionSeq}`;
    const msg = {
      t: "rx",
      a: this.selfAddr,
      n: this.selfName,
      c: this.colors.color,
      cl: this.colors.light,
      e,
      x,
      y,
      i: id,
    };
    this.sendJson(msg);
    // Echo locally so sender sees it immediately.
    this.emitReaction({
      id,
      addr: this.selfAddr,
      name: this.selfName,
      color: this.colors.color,
      emoji: e,
      x,
      y,
      at: Date.now(),
    });
  }

  /**
   * @param {Uint8Array | ArrayLike<number> | number[]} data
   * @returns {boolean} true if consumed as presence
   */
  handleMessage(data) {
    if (!data || !data.length) return false;
    // Normalize — desktop delivers Uint8Array; some bridges hand number[].
    const bytes =
      data instanceof Uint8Array
        ? data
        : Uint8Array.from(/** @type {ArrayLike<number>} */ (data));
    if (bytes[0] !== 0x7b /* '{' */) return false;
    let msg;
    try {
      msg = JSON.parse(new TextDecoder().decode(bytes));
    } catch {
      return true;
    }
    if (!msg?.t || typeof msg.a !== "string" || !msg.a) return true;
    // Colors are derived locally; never trust peer-supplied CSS.
    const safeColors = pickUserColor(msg.a);
    msg.c = safeColors.color;
    msg.cl = safeColors.light;

    // Ignore our own frames (including host-relayed copies stamped with h:1).
    if (msg.a === this.selfAddr) return true;

    // Reactions — de-dupe when both direct gossip + host relay arrive.
    if (msg.t === "rx") {
      const id = msg.i || `${msg.a}-${msg.x}-${msg.y}-${msg.e}`;
      const now = Date.now();
      if (this.seenRx.has(id)) return true;
      this.seenRx.set(id, now + 8_000);
      this.emitReaction({
        id,
        addr: msg.a,
        name: msg.n || msg.a,
        color: msg.c || pickUserColor(msg.a).color,
        emoji: (msg.e || "").slice(0, 16),
        x: Number(msg.x) || 0,
        y: Number(msg.y) || 0,
        at: now,
      });
    }

    const colors = pickUserColor(msg.a);
    const peer = this.peers.get(msg.a) ?? {
      addr: msg.a,
      name: msg.n || msg.a,
      color: msg.c || colors.color,
      colorLight: msg.cl || colors.light,
      lastSeen: Date.now(),
    };
    peer.name = msg.n || peer.name;
    peer.color = msg.c || peer.color;
    peer.colorLight = msg.cl || peer.colorLight;
    peer.lastSeen = Date.now();

    if (msg.t === "leave") {
      this.peers.delete(msg.a);
      this.emit();
      return true;
    }

    if (msg.t === "pos") {
      const x = Number(msg.x);
      const y = Number(msg.y);
      if (Number.isFinite(x) && Number.isFinite(y)) {
        peer.pointer = {
          x,
          y,
          button: msg.b === "down" ? "down" : "up",
        };
        peer.pointerUpdatedAt = Date.now();
      }
    } else if (msg.t === "writer" || msg.t === "in") {
      peer.writing = true;
      peer.writingUpdatedAt = Date.now();
    }

    this.peers.set(msg.a, peer);
    this.emit();
    return true;
  }

  gc() {
    const now = Date.now();
    let changed = false;
    for (const [addr, peer] of this.peers) {
      if (now - peer.lastSeen > STALE_MS) {
        this.peers.delete(addr);
        changed = true;
        continue;
      }
      if (peer.pointer && now - (peer.pointerUpdatedAt || 0) > POINTER_STALE_MS) {
        peer.pointer = undefined;
        changed = true;
      }
      if (peer.writing && now - (peer.writingUpdatedAt || 0) > WRITER_STALE_MS) {
        peer.writing = false;
        changed = true;
      }
    }
    for (const [id, exp] of this.seenRx) {
      if (exp < now) this.seenRx.delete(id);
    }
    if (changed) this.emit();
  }
}
