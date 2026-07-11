/**
 * Soft-key strip above the OS keyboard (phone).
 * Sends terminal sequences via onSend(string).
 *
 * @param {{
 *   visible: boolean,
 *   onSend: (s: string) => void,
 *   ctrlHeld: boolean,
 *   onCtrlToggle: () => void,
 * }} props
 */
export function MobileExtraKeys({ visible, onSend, ctrlHeld, onCtrlToggle }) {
  if (!visible) return null;

  const fire = (s) => (e) => {
    e.preventDefault();
    e.stopPropagation();
    onSend(s);
  };

  // Prevent buttons from stealing focus from xterm (keeps keyboard open on iOS).
  const keepFocus = (e) => {
    e.preventDefault();
  };

  return (
    <div id="extra-keys" class="extra-keys" role="toolbar" aria-label="Terminal keys">
      <button
        type="button"
        class={`extra-key extra-key--toggle ${ctrlHeld ? "is-on" : ""}`}
        onPointerDown={keepFocus}
        onClick={(e) => {
          e.preventDefault();
          onCtrlToggle();
        }}
      >
        Ctrl
      </button>
      <button type="button" class="extra-key" onPointerDown={keepFocus} onClick={fire("\x1b")}>
        Esc
      </button>
      <button type="button" class="extra-key" onPointerDown={keepFocus} onClick={fire("\t")}>
        Tab
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\x03")}
        title="Ctrl-C"
      >
        ^C
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\x04")}
        title="Ctrl-D"
      >
        ^D
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\x1a")}
        title="Ctrl-Z"
      >
        ^Z
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\x1b[A")}
        title="Up"
      >
        ↑
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\x1b[B")}
        title="Down"
      >
        ↓
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\x1b[D")}
        title="Left"
      >
        ←
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\x1b[C")}
        title="Right"
      >
        →
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\x1b[H")}
        title="Home"
      >
        Home
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\x1b[F")}
        title="End"
      >
        End
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\x7f")}
        title="Backspace"
      >
        ⌫
      </button>
      <button
        type="button"
        class="extra-key"
        onPointerDown={keepFocus}
        onClick={fire("\r")}
        title="Enter"
      >
        ↵
      </button>
    </div>
  );
}
