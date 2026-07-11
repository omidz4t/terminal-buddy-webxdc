import { useEffect, useRef } from "preact/hooks";

/**
 * Imperative peer cursors — avoid Preact re-render at ~60Hz (presence pos).
 * Parent still owns the peers list; we only patch this overlay's DOM.
 *
 * @param {{ peers: Array<any> }} props
 */
export function PeerCursors({ peers }) {
  const rootRef = useRef(/** @type {HTMLDivElement | null} */ (null));
  const nodesRef = useRef(/** @type {Map<string, HTMLDivElement>} */ (new Map()));

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;

    const withPtr = peers.filter(
      (p) => p.pointer && Number.isFinite(p.pointer.x),
    );
    const wanted = new Set(withPtr.map((p) => p.addr));
    const nodes = nodesRef.current;

    for (const [addr, el] of nodes) {
      if (!wanted.has(addr)) {
        el.remove();
        nodes.delete(addr);
      }
    }

    for (const p of withPtr) {
      const x = p.pointer.x || 0;
      const y = p.pointer.y || 0;
      const down = p.pointer.button === "down";
      const color = p.color || "#7ee787";
      let el = nodes.get(p.addr);
      if (!el) {
        el = document.createElement("div");
        el.className = "peer-cursor";
        el.dataset.addr = p.addr;
        el.innerHTML = `
          <div class="ring" hidden></div>
          <svg class="arrow" width="12" height="15" viewBox="0 0 12 15" fill="none">
            <path d="M0 0 L0 14 L4 9 L11 8 Z" fill="#fff" stroke="#fff" stroke-width="3" stroke-linejoin="round"/>
            <path class="arrow-fill" d="M0 0 L0 14 L4 9 L11 8 Z" fill="${color}" stroke="${color}" stroke-width="1.5" stroke-linejoin="round"/>
          </svg>
          <span class="label"></span>`;
        root.appendChild(el);
        nodes.set(p.addr, el);
      }
      el.style.transform = `translate(${x}px,${y}px)`;
      el.style.setProperty("--cursor-color", color);
      const ring = el.querySelector(".ring");
      if (ring instanceof HTMLElement) ring.hidden = !down;
      const label = el.querySelector(".label");
      if (label) {
        const text = p.name || p.addr;
        if (label.textContent !== text) label.textContent = text;
      }
      const fill = el.querySelector(".arrow-fill");
      if (fill) {
        fill.setAttribute("fill", color);
        fill.setAttribute("stroke", color);
      }
    }
  }, [peers]);

  return <div id="cursors" ref={rootRef} aria-hidden="true" />;
}
