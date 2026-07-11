/**
 * @param {{ bursts: Array<{ id: string, emoji: string, name?: string, color?: string, x: number, y: number }> }} props
 */
export function ReactionBursts({ bursts }) {
  return (
    <div id="reaction-bursts" aria-hidden="true">
      {bursts.map((r) => (
        <div
          key={r.id}
          class="reaction-burst"
          style={{
            "--tx": `${r.x || 0}px`,
            "--ty": `${r.y || 0}px`,
            "--rx-color": r.color || "#7ee787",
          }}
        >
          <span class="reaction-burst__emoji">{r.emoji}</span>
          <span class="reaction-burst__name">{r.name || ""}</span>
        </div>
      ))}
    </div>
  );
}
