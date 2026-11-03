import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { installClipboardHandler } from "./clipboard.js";
import { ClipboardRequest } from "./components/ClipboardRequest.jsx";
import { Presence, pickUserColor } from "./presence.js";
import { useWebxdc } from "./hooks/useWebxdc.js";
import { useVisualViewport } from "./hooks/useVisualViewport.js";
import { Titlebar } from "./components/Titlebar.jsx";
import { TerminalHost } from "./components/TerminalHost.jsx";
import { PeerCursors } from "./components/PeerCursors.jsx";
import { ReactionBursts } from "./components/ReactionBursts.jsx";
import { ReactionPie, PIE_RADIUS } from "./components/ReactionPie.jsx";
import { MobileExtraKeys } from "./components/MobileExtraKeys.jsx";

/**
 * Realtime protocol (shared mesh; roles matter):
 *
 * Peers propagate what they want others/host to see:
 *   I…  key input          → host only (applied to PTY)
 *   R…  resize             → host only (PTY geometry)
 *   P   ping               → host (advertisement / pong)
 *   {…} presence JSON      → all peers naturally (Iroh gossip)
 *       join | leave | pos | writer | in | rx
 *
 * Host alone sends terminal content back:
 *   O…  PTY output
 *   E   shell exited
 *
 * Cursors (`pos`) and reactions (`rx`) are peer→peer; host does not relay them.
 * Alt + mouse move opens the reaction pie.
 */

const MOVE_THRESHOLD_PX = 8;
const OPEN_DELAY_MS = 60;
const BURST_MS = 2200;

function normalizeBytes(data) {
  if (!data || !data.length) return null;
  if (data instanceof Uint8Array) return data;
  if (ArrayBuffer.isView(data)) {
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  }
  return Uint8Array.from(data);
}

function isAltKey(e) {
  return e.key === "Alt" || e.code === "AltLeft" || e.code === "AltRight";
}

function clampPieClient(x, y) {
  const pad = 16;
  const half = PIE_RADIUS + 42;
  return {
    x: Math.min(
      Math.max(x, pad + half),
      Math.max(pad + half, window.innerWidth - pad - half),
    ),
    y: Math.min(
      Math.max(y, pad + half),
      Math.max(pad + half, window.innerHeight - pad - half),
    ),
  };
}

export function App() {
  // Lift/resize UI above the soft keyboard (phone).
  useVisualViewport();
  const { webxdc, error, ready } = useWebxdc();
  const [clipboardRequest, setClipboardRequest] = useState(null);
  const [status, setStatus] = useState("loading…");
  const [statusOk, setStatusOk] = useState(false);
  const [peers, setPeers] = useState(/** @type {any[]} */ ([]));
  const [bursts, setBursts] = useState(/** @type {any[]} */ ([]));
  const [pieOpen, setPieOpen] = useState(false);
  const [pieClient, setPieClient] = useState({ x: 0, y: 0 });
  const [altHeld, setAltHeld] = useState(false);
  const [setupError, setSetupError] = useState(
    /** @type {string | null} */ (null),
  );
  /** Host approve-mode: waiting for host to allow a command */
  const [cmdWait, setCmdWait] = useState(
    /** @type {{ id: string, line: string } | null} */ (null),
  );
  const [extraKeysEnabled, setExtraKeysEnabled] = useState(() => {
    try {
      return localStorage.getItem("tb.extraKeys") === "1";
    } catch {
      return false;
    }
  });
  const [ctrlHeld, setCtrlHeld] = useState(false);

  const stageRef = useRef(/** @type {HTMLDivElement | null} */ (null));
  const termHostRef = useRef(/** @type {HTMLDivElement | null} */ (null));
  const requestClipboardRef = useRef(null);
  const clipboardTimerRef = useRef(null);
  const presenceRef = useRef(/** @type {Presence | null} */ (null));
  /** Inject soft-key sequences into the same path as keyboard input */
  const sendInputRef = useRef(/** @type {null | ((s: string) => void)} */ (null));
  const termRef = useRef(/** @type {import('@xterm/xterm').Terminal | null} */ (null));
  const connectedRef = useRef(false);
  const ctrlHeldRef = useRef(false);
  /** rAF coalescing for peer list → avoid setState storms from pos frames */
  const peersRaf = useRef(0);
  const peersPending = useRef(/** @type {any[] | null} */ (null));

  // Mutable gesture state (high-frequency; not worth re-rendering each move).
  const gesture = useRef({
    pointerDown: false,
    lastClient: { x: window.innerWidth / 2, y: window.innerHeight / 2 },
    altHeld: false,
    altArmOrigin: /** @type {{ x: number, y: number } | null} */ (null),
    chordUsed: false,
    pieOpen: false,
    pieClient: { x: 0, y: 0 },
    openTimer: /** @type {ReturnType<typeof setTimeout> | null} */ (null),
  });

  const setStatusPair = useCallback((text, ok) => {
    setStatus(text);
    setStatusOk(!!ok);
  }, []);

  const toggleExtraKeys = useCallback(() => {
    setExtraKeysEnabled((v) => {
      const next = !v;
      try {
        localStorage.setItem("tb.extraKeys", next ? "1" : "0");
      } catch {
        /* ignore */
      }
      return next;
    });
  }, []);

  const toggleCtrl = useCallback(() => {
    setCtrlHeld((v) => {
      ctrlHeldRef.current = !v;
      return !v;
    });
  }, []);

  /** Soft keys / menu: send sequence, applying sticky Ctrl if set. */
  const injectKeys = useCallback((raw) => {
    let s = raw;
    if (ctrlHeldRef.current && s.length === 1) {
      const c = s.toLowerCase();
      if (c >= "a" && c <= "z") {
        s = String.fromCharCode(c.charCodeAt(0) & 0x1f);
      }
      ctrlHeldRef.current = false;
      setCtrlHeld(false);
    }
    sendInputRef.current?.(s);
    try {
      termRef.current?.focus();
    } catch {
      /* ignore */
    }
  }, []);

  const schedulePeers = useCallback((list) => {
    peersPending.current = list;
    if (peersRaf.current) return;
    peersRaf.current = requestAnimationFrame(() => {
      peersRaf.current = 0;
      const next = peersPending.current;
      peersPending.current = null;
      if (next) setPeers(next);
    });
  }, []);

  const pushBurst = useCallback((reaction) => {
    if (!reaction?.emoji) return;
    const id = reaction.id || `${reaction.addr}-${Date.now()}`;
    const item = { ...reaction, id };
    setBursts((prev) => [...prev, item]);
    setTimeout(() => {
      setBursts((prev) => prev.filter((b) => b.id !== id));
    }, BURST_MS);
  }, []);

  const stageLocal = useCallback((clientX, clientY) => {
    const el = stageRef.current;
    if (!el) return { x: clientX, y: clientY };
    const rect = el.getBoundingClientRect();
    return { x: clientX - rect.left, y: clientY - rect.top };
  }, []);

  const closePie = useCallback(() => {
    const g = gesture.current;
    if (g.openTimer) {
      clearTimeout(g.openTimer);
      g.openTimer = null;
    }
    g.pieOpen = false;
    setPieOpen(false);
  }, []);

  const openPieAtCursor = useCallback(() => {
    const g = gesture.current;
    if (g.openTimer) {
      clearTimeout(g.openTimer);
      g.openTimer = null;
    }
    const pos = clampPieClient(g.lastClient.x, g.lastClient.y);
    g.pieClient = pos;
    g.pieOpen = true;
    setPieClient(pos);
    setPieOpen(true);
  }, []);

  const pickReaction = useCallback(
    (emoji) => {
      const g = gesture.current;
      const coords = stageLocal(g.pieClient.x, g.pieClient.y);
      closePie();
      presenceRef.current?.sendReaction(emoji, coords.x, coords.y);
    },
    [closePie, stageLocal],
  );

  // Body classes for cursor styling — only while Alt / pie (not on every move).
  useEffect(() => {
    document.body.classList.toggle("alt-reaction", altHeld);
  }, [altHeld]);

  useEffect(() => {
    document.body.classList.toggle("reaction-picking", pieOpen);
  }, [pieOpen]);

  useEffect(() => {
    if (!ready) return;
    if (error) {
      setSetupError(error.message || String(error));
      setStatusPair("setup failed", false);
    }
  }, [ready, error, setStatusPair]);

  // Boot terminal + realtime once webxdc is ready.
  useEffect(() => {
    if (!webxdc || !termHostRef.current) return;

    const selfAddr = webxdc.selfAddr || "unknown";
    const selfName = webxdc.selfName || selfAddr;
    const selfColors = pickUserColor(selfAddr);

    const term = new Terminal({
      cursorBlink: true,
      fontFamily:
        'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace',
      fontSize: 13,
      theme: {
        background: "#0b0f14",
        foreground: "#e6edf3",
        cursor: "#7ee787",
        selectionBackground: "#264f78",
      },
      convertEol: false,
      scrollback: 10000,
      allowProposedApi: true,
      macOptionIsMeta: false,
    });
    const clipboardHandler = installClipboardHandler(term, (text) => {
      setClipboardRequest({ text, id: Date.now() });
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(termHostRef.current);
    fitAddon.fit();
    // Never swallow keys — let Ctrl-b and all tmux sequences reach the PTY.
    term.attachCustomKeyEventHandler(() => true);
    termRef.current = term;

    const realtime = webxdc.joinRealtimeChannel();
    const sendRaw = (u8) => {
      try {
        const bytes = u8 instanceof Uint8Array ? u8 : Uint8Array.from(u8);
        realtime.send(bytes);
      } catch (e) {
        console.error("realtime.send", e);
      }
    };

    requestClipboardRef.current = () => {
      const addr = new TextEncoder().encode(selfAddr);
      if (addr.length > 512) { setStatusPair("Clipboard address too long", false); return; }
      const frame = new Uint8Array(1 + addr.length);
      frame[0] = 0x43; // C: explicit tmux-buffer request
      frame.set(addr, 1);
      sendRaw(frame);
      clearTimeout(clipboardTimerRef.current);
      clipboardTimerRef.current = setTimeout(() => {
        setStatusPair("No clipboard reply — check connection and host version", false);
      }, 6000);
      setStatusPair("requesting tmux buffer…", false);
    };

    const presence = new Presence(selfAddr, selfName, sendRaw);
    presenceRef.current = presence;
    const offPeers = presence.onChange((list) => schedulePeers(list));
    const offRx = presence.onReaction((r) => pushBurst(r));

    connectedRef.current = false;

    realtime.setListener((data) => {
      const msg = normalizeBytes(data);
      if (!msg) return;

      // Host command-gate messages (approve mode) + peer presence JSON.
      if (msg[0] === 0x7b /* '{' */) {
        try {
          const j = JSON.parse(new TextDecoder().decode(msg));
          if (j?.t === "clipboard_reply") {
            if (j.a !== selfAddr) return;
            clearTimeout(clipboardTimerRef.current);
            if (typeof j.text === "string" && j.text.length <= 16384) {
              setClipboardRequest({ text: j.text, id: Date.now() });
              setStatusPair("tmux buffer ready to copy", connectedRef.current);
            } else {
              setStatusPair(typeof j.error === "string" ? j.error : "Unable to read tmux buffer", false);
            }
            return;
          }
          if (j?.t === "cmd_wait") {
            setCmdWait({
              id: String(j.i || ""),
              line: String(j.line || j.cmd || "…"),
            });
            setStatusPair("waiting for approval…", false);
            return;
          }
          if (j?.t === "cmd_done") {
            setCmdWait(null);
            setStatusPair(j.ok ? "connected" : "command denied", !!j.ok);
            return;
          }
        } catch {
          /* fall through to presence */
        }
        if (presence.handleMessage(msg)) return;
        return;
      }

      const kind = msg[0];
      // Only the host publishes the terminal stream.
      if (kind === 0x4f /* O */) {
        if (!connectedRef.current) {
          connectedRef.current = true;
          setStatusPair("connected", true);
        }
        term.write(msg.subarray(1));
      } else if (kind === 0x45 /* E */) {
        connectedRef.current = false;
        setStatusPair("shell exited", false);
      }
    });

    const sendInput = (str) => {
      // Sticky Ctrl from soft-key bar: next letter becomes control char.
      let payload = str;
      if (ctrlHeldRef.current && payload.length === 1) {
        const c = payload.toLowerCase();
        if (c >= "a" && c <= "z") {
          payload = String.fromCharCode(c.charCodeAt(0) & 0x1f);
          ctrlHeldRef.current = false;
          setCtrlHeld(false);
        }
      }
      presence.announceWriter();
      // Frame: I | 0x01 | u16le addr_len | addr utf8 | key payload
      // Versioned so hashed selfAddr never collides with raw keys in the PTY.
      const addrBytes = new TextEncoder().encode(selfAddr || "");
      const keyBytes = new TextEncoder().encode(payload);
      const addrLen = Math.min(addrBytes.length, 512);
      const buf = new Uint8Array(1 + 1 + 2 + addrLen + keyBytes.length);
      let o = 0;
      buf[o++] = 0x49; // I
      buf[o++] = 0x01; // framing version
      buf[o++] = addrLen & 0xff;
      buf[o++] = (addrLen >> 8) & 0xff;
      buf.set(addrBytes.subarray(0, addrLen), o);
      o += addrLen;
      buf.set(keyBytes, o);
      sendRaw(buf);
      sendRaw(
        new TextEncoder().encode(
          JSON.stringify({
            t: "in",
            a: selfAddr,
            n: selfName,
            c: selfColors.color,
          }),
        ),
      );
    };
    sendInputRef.current = sendInput;
    term.onData(sendInput);

    // ── fit / resize (no ResizeObserver feedback loop) ───────────────────
    let lastCols = 0;
    let lastRows = 0;
    let lastBoxW = -1;
    let lastBoxH = -1;
    let fitRaf = 0;
    let fitRetry = 0;

    const applyFit = (forceSend = false) => {
      const host = termHostRef.current;
      if (!host) return;
      const w = host.clientWidth;
      const h = host.clientHeight;
      if (w < 2 || h < 2) return;

      // Skip work when the box is unchanged (kills RO → fit → RO loops).
      if (!forceSend && w === lastBoxW && h === lastBoxH) {
        if (lastCols > 0) return;
      }
      lastBoxW = w;
      lastBoxH = h;

      try {
        fitAddon.fit();
      } catch (e) {
        console.warn("fitAddon.fit", e);
        return;
      }

      const cols = term.cols | 0;
      const rows = term.rows | 0;
      if (cols < 2 || rows < 1) return;
      if (!forceSend && cols === lastCols && rows === lastRows) return;
      lastCols = cols;
      lastRows = rows;

      // Only full refresh when the cell grid actually changes.
      try {
        term.refresh(0, term.rows - 1);
      } catch {
        /* ignore */
      }

      const buf = new Uint8Array(5);
      buf[0] = 0x52;
      buf[1] = cols & 0xff;
      buf[2] = (cols >> 8) & 0xff;
      buf[3] = rows & 0xff;
      buf[4] = (rows >> 8) & 0xff;
      sendRaw(buf);
    };

    const scheduleFit = (forceSend = false) => {
      if (fitRaf) cancelAnimationFrame(fitRaf);
      if (fitRetry) cancelAnimationFrame(fitRetry);
      fitRaf = requestAnimationFrame(() => {
        fitRaf = 0;
        applyFit(forceSend);
        // One extra pass after layout settles (window chrome).
        if (forceSend) {
          fitRetry = requestAnimationFrame(() => {
            fitRetry = 0;
            applyFit(true);
          });
        }
      });
    };

    const onWinResize = () => scheduleFit(true);
    window.addEventListener("resize", onWinResize);
    window.visualViewport?.addEventListener("resize", onWinResize);

    // Observe only the stage — not the xterm host (fit mutates host children).
    let ro = /** @type {ResizeObserver | null} */ (null);
    if (typeof ResizeObserver !== "undefined" && stageRef.current) {
      ro = new ResizeObserver((entries) => {
        const cr = entries[0]?.contentRect;
        if (!cr) {
          scheduleFit(false);
          return;
        }
        const w = Math.round(cr.width);
        const h = Math.round(cr.height);
        if (w === lastBoxW && h === lastBoxH) return;
        scheduleFit(false);
      });
      ro.observe(stageRef.current);
    }

    const ping = (label) => {
      sendRaw(new Uint8Array([0x50]));
      presence.announceJoin();
      if (label) setStatusPair(label, false);
    };

    // Lifecycle events (joined / ping / connected) live in the header title only.
    setStatusPair("joined realtime", false);

    scheduleFit(true);
    // Brief event trail: joined → ping → waiting…
    setTimeout(() => {
      if (!connectedRef.current) ping("ping…");
    }, 280);

    let n = 0;
    const iv = setInterval(() => {
      if (connectedRef.current) {
        clearInterval(iv);
        scheduleFit(true);
        return;
      }
      n += 1;
      if (n > 40) {
        clearInterval(iv);
        setStatusPair("no host yet", false);
        return;
      }
      ping(n % 2 === 0 ? `waiting for host… ${n}` : "ping…");
      scheduleFit(true);
    }, 1500);

    term.focus();

    const onBeforeUnload = () => presence.destroy();
    window.addEventListener("beforeunload", onBeforeUnload);

    return () => {
      clearInterval(iv);
      if (fitRaf) cancelAnimationFrame(fitRaf);
      if (fitRetry) cancelAnimationFrame(fitRetry);
      if (peersRaf.current) cancelAnimationFrame(peersRaf.current);
      window.removeEventListener("resize", onWinResize);
      window.visualViewport?.removeEventListener("resize", onWinResize);
      window.removeEventListener("beforeunload", onBeforeUnload);
      ro?.disconnect();
      offPeers();
      offRx();
      presence.destroy();
      clearTimeout(clipboardTimerRef.current);
      requestClipboardRef.current = null;
      presenceRef.current = null;
      sendInputRef.current = null;
      termRef.current = null;
      try {
        realtime.leave?.();
      } catch {
        /* ignore */
      }
      clipboardHandler.dispose();
      term.dispose();
    };
  }, [webxdc, pushBurst, schedulePeers, setStatusPair]);

  // ── Pointer / Alt reaction gestures ────────────────────────────────────
  useEffect(() => {
    const g = gesture.current;

    const clearOpenTimer = () => {
      if (g.openTimer) {
        clearTimeout(g.openTimer);
        g.openTimer = null;
      }
    };

    const scheduleOpenPie = () => {
      if (g.pieOpen || g.openTimer || g.chordUsed || !g.altHeld) return;
      g.openTimer = setTimeout(() => {
        g.openTimer = null;
        if (g.altHeld && !g.chordUsed && !g.pieOpen) openPieAtCursor();
      }, OPEN_DELAY_MS);
    };

    const cancelForChord = () => {
      g.chordUsed = true;
      g.altHeld = false;
      g.altArmOrigin = null;
      setAltHeld(false);
      closePie();
    };

    const onKeyDown = (e) => {
      if (isAltKey(e) && !e.ctrlKey && !e.metaKey && !e.shiftKey && !e.repeat) {
        g.altHeld = true;
        g.chordUsed = false;
        g.altArmOrigin = { ...g.lastClient };
        setAltHeld(true);
        return;
      }
      if ((e.altKey || g.altHeld) && !isAltKey(e)) {
        cancelForChord();
        return;
      }
      if (e.key === "Escape" && g.pieOpen) {
        e.preventDefault();
        e.stopPropagation();
        closePie();
      }
    };

    const onKeyUp = (e) => {
      if (isAltKey(e)) {
        g.altHeld = false;
        g.altArmOrigin = null;
        setAltHeld(false);
        if (g.chordUsed) closePie();
        else clearOpenTimer();
        return;
      }
      if (e.altKey || g.altHeld || g.openTimer != null) cancelForChord();
    };

    const onPointerMove = (e) => {
      g.lastClient = { x: e.clientX, y: e.clientY };

      if (!g.pieOpen && presenceRef.current && stageRef.current) {
        const { x, y } = stageLocal(e.clientX, e.clientY);
        presenceRef.current.updatePointer(
          x,
          y,
          g.pointerDown || e.buttons ? "down" : "up",
        );
      }

      if (!g.altHeld || g.chordUsed || g.pieOpen) return;
      if (e.buttons !== 0) return;
      if (g.altArmOrigin) {
        const dist = Math.hypot(
          e.clientX - g.altArmOrigin.x,
          e.clientY - g.altArmOrigin.y,
        );
        if (dist < MOVE_THRESHOLD_PX) return;
      }
      scheduleOpenPie();
    };

    const onPointerDown = (e) => {
      const t = e.target;
      if (g.pieOpen && t instanceof Element && t.closest(".reaction-wedge")) {
        return;
      }

      g.pointerDown = true;
      if (!g.pieOpen && presenceRef.current) {
        const { x, y } = stageLocal(e.clientX, e.clientY);
        presenceRef.current.updatePointer(x, y, "down", { immediate: true });
      }

      if (!g.pieOpen) return;
      if (t instanceof Element && t.closest(".reaction-pie")) return;
      closePie();
    };

    const onPointerUp = (e) => {
      g.pointerDown = false;
      if (presenceRef.current) {
        const { x, y } = stageLocal(e.clientX, e.clientY);
        presenceRef.current.updatePointer(x, y, "up", { immediate: true });
      }
    };

    const onBlur = () => {
      g.altHeld = false;
      g.pointerDown = false;
      setAltHeld(false);
      closePie();
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("pointermove", onPointerMove, { passive: true });
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("pointerup", onPointerUp, true);
    window.addEventListener("blur", onBlur);

    return () => {
      clearOpenTimer();
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("pointerup", onPointerUp, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [closePie, openPieAtCursor, stageLocal]);

  return (
    <>
      <Titlebar
        status={status}
        statusOk={statusOk}
        extraKeysEnabled={extraKeysEnabled}
        onToggleExtraKeys={toggleExtraKeys}
        onCopyTmuxBuffer={() => requestClipboardRef.current?.()}
      />
      <div id="main">
        <div id="stage" ref={stageRef}>
          {/* Never re-rendered — xterm owns this DOM subtree. */}
          <TerminalHost hostRef={termHostRef} />
          {setupError ? (
            <pre class="setup-error setup-error--overlay">
              {setupError}
              {"\n\n"}
              Need &lt;script src="webxdc.js"&gt; (provided by Delta Chat).
            </pre>
          ) : null}
          {cmdWait ? (
            <div class="cmd-wait-overlay" role="status" aria-live="polite">
              <div class="cmd-wait-card">
                <div class="cmd-wait-title">Waiting for host approval</div>
                <div class="cmd-wait-cmd">
                  <code>{cmdWait.line}</code>
                </div>
                <div class="cmd-wait-hint">
                  The host must press <kbd>Y</kbd> or <kbd>n</kbd> on Shell tab
                </div>
              </div>
            </div>
          ) : null}
          <PeerCursors peers={peers} />
          <ReactionBursts bursts={bursts} />
        </div>
      </div>
      {clipboardRequest ? (
        <ClipboardRequest key={clipboardRequest.id} text={clipboardRequest.text}
          onClose={() => { setClipboardRequest(null); termRef.current?.focus(); }} />
      ) : null}
      <MobileExtraKeys
        visible={extraKeysEnabled}
        onSend={injectKeys}
        ctrlHeld={ctrlHeld}
        onCtrlToggle={toggleCtrl}
      />
      <ReactionPie
        open={pieOpen}
        x={pieClient.x}
        y={pieClient.y}
        onPick={pickReaction}
      />
    </>
  );
}
