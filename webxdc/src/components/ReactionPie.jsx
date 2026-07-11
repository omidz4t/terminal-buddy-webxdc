const REACTIONS = ["👍", "❤️", "😂", "😮", "😢", "🎉", "🔥", "👀"];
export const PIE_RADIUS = 78;

/**
 * @param {{
 *   open: boolean,
 *   x: number,
 *   y: number,
 *   onPick: (emoji: string) => void,
 * }} props
 */
export function ReactionPie({ open, x, y, onPick }) {
  const n = REACTIONS.length;

  return (
    <div
      id="reaction-pie"
      class="reaction-pie"
      hidden={!open}
      style={open ? { left: `${x}px`, top: `${y}px` } : undefined}
    >
      <div class="reaction-pie__ring" />
      <div class="reaction-pie__items" id="reaction-pie-items">
        {REACTIONS.map((emoji, i) => {
          const angle = -Math.PI / 2 + (i * 2 * Math.PI) / n;
          const sx = Math.cos(angle) * PIE_RADIUS;
          const sy = Math.sin(angle) * PIE_RADIUS;
          return (
            <div
              key={emoji}
              class="reaction-slot"
              style={{ transform: `translate(${sx}px,${sy}px)` }}
            >
              <button
                type="button"
                class="reaction-wedge"
                data-emoji={emoji}
                tabIndex={0}
                onPointerDown={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                }}
                onPointerUp={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  onPick(emoji);
                }}
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  onPick(emoji);
                }}
              >
                {emoji}
              </button>
            </div>
          );
        })}
      </div>
      <div class="reaction-pie__hint">click emoji · Esc cancel</div>
    </div>
  );
}
