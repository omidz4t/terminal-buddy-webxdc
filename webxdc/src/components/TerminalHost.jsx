import { memo } from "preact/compat";

/**
 * Never re-render after mount. xterm mutates this node's children; if Preact
 * reconciles it (e.g. on every peer-cursor setState), it wipes the terminal
 * and the whole view — including the mouse — flickers.
 *
 * @param {{ hostRef: import('preact').Ref<HTMLDivElement | null> }} props
 */
export const TerminalHost = memo(
  function TerminalHost({ hostRef }) {
    return <div id="terminal" ref={hostRef} />;
  },
  () => true,
);
