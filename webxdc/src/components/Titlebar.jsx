import { useEffect, useRef, useState } from "preact/hooks";

/**
 * Header with sliding status title (overflow hidden).
 * Events like ping / connected replace the title with a vertical slide.
 *
 * @param {{
 *   status: string,
 *   statusOk: boolean,
 *   extraKeysEnabled: boolean,
 *   onToggleExtraKeys: () => void,
 *   onCopyTmuxBuffer: () => void,
 * }} props
 */
export function Titlebar({
  status,
  statusOk,
  extraKeysEnabled,
  onToggleExtraKeys,
  onCopyTmuxBuffer,
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const wrapRef = useRef(/** @type {HTMLDivElement | null} */ (null));

  const [shown, setShown] = useState({ text: status, ok: statusOk, key: 0 });
  const [leaving, setLeaving] = useState(false);

  const currentText = useRef(status);
  const busy = useRef(false);
  const queue = useRef(/** @type {{ text: string, ok: boolean } | null} */ (null));
  const timers = useRef(/** @type {ReturnType<typeof setTimeout>[]} */ ([]));

  const clearTimers = () => {
    for (const t of timers.current) clearTimeout(t);
    timers.current = [];
  };

  const runSlide = (next) => {
    busy.current = true;
    clearTimers();
    setLeaving(true);
    timers.current.push(
      setTimeout(() => {
        currentText.current = next.text;
        setShown((s) => ({ text: next.text, ok: next.ok, key: s.key + 1 }));
        setLeaving(false);
        timers.current.push(
          setTimeout(() => {
            busy.current = false;
            const q = queue.current;
            queue.current = null;
            if (q && q.text !== currentText.current) {
              runSlide(q);
            } else if (q) {
              setShown((s) => ({ ...s, ok: q.ok }));
            }
          }, 240),
        );
      }, 180),
    );
  };

  useEffect(() => {
    if (status === currentText.current) {
      setShown((s) => (s.ok === statusOk ? s : { ...s, ok: statusOk }));
      return;
    }
    if (busy.current) {
      queue.current = { text: status, ok: statusOk };
      return;
    }
    runSlide({ text: status, ok: statusOk });
  }, [status, statusOk]);

  useEffect(() => () => clearTimers(), []);

  useEffect(() => {
    if (!menuOpen) return;
    const onDoc = (e) => {
      if (wrapRef.current && !wrapRef.current.contains(/** @type {Node} */ (e.target))) {
        setMenuOpen(false);
      }
    };
    document.addEventListener("pointerdown", onDoc, true);
    return () => document.removeEventListener("pointerdown", onDoc, true);
  }, [menuOpen]);

  return (
    <header id="titlebar">
      <div class="titlebar-brand" aria-hidden="true">
        TB
      </div>

      <div
        class="titlebar-title-slot"
        title={shown.text}
        role="status"
        aria-live="polite"
      >
        <span
          key={shown.key}
          class={`titlebar-title-slide${leaving ? " is-out" : " is-in"}`}
          data-state={shown.ok ? "ok" : "wait"}
          id="status"
        >
          {shown.text || "TerminalBuddy"}
        </span>
      </div>

      <div class="titlebar-menu-wrap" ref={wrapRef}>
        <button
          type="button"
          class="titlebar-menu-btn"
          aria-label="Menu"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((v) => !v)}
        >
          ⋮
        </button>
        {menuOpen ? (
          <div class="titlebar-menu" role="menu">
            <button type="button" class="titlebar-menu-item" role="menuitem"
              onClick={() => { setMenuOpen(false); onCopyTmuxBuffer(); }}>
              <span class="titlebar-menu-check" />
              Copy tmux buffer
              <span class="titlebar-menu-sub">Retrieve text copied by terminal tools</span>
            </button>
            <button
              type="button"
              class="titlebar-menu-item"
              role="menuitemcheckbox"
              aria-checked={extraKeysEnabled}
              onClick={() => {
                onToggleExtraKeys();
              }}
            >
              <span class="titlebar-menu-check">
                {extraKeysEnabled ? "✓" : ""}
              </span>
              Extra keys bar
              <span class="titlebar-menu-sub">
                Ctrl, Tab, Esc, arrows above keyboard
              </span>
            </button>
            <button
              type="button"
              class="titlebar-menu-item"
              role="menuitem"
              onClick={() => {
                setMenuOpen(false);
                try {
                  document
                    .getElementById("terminal")
                    ?.querySelector("textarea,canvas")
                    ?.focus?.();
                } catch {
                  /* ignore */
                }
              }}
            >
              <span class="titlebar-menu-check" />
              Focus terminal
            </button>
            <button
              type="button"
              class="titlebar-menu-item"
              role="menuitem"
              onClick={() => setMenuOpen(false)}
            >
              <span class="titlebar-menu-check" />
              Close menu
            </button>
          </div>
        ) : null}
      </div>
    </header>
  );
}
