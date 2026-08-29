export interface DonutSegment {
  label: string;
  value: number;
  colour: string;
}

/** What a non-hovered slice keeps of its colour — the chart's `DIM_ALPHA`, so
 *  the two hover states in the system look like one. */
const DIM_OPACITY = 0.35;

/**
 * The ring the artboards draw, in plain SVG — a stroked circle per segment with
 * `stroke-dasharray` and a cumulative offset. No library: one arc renderer is
 * not worth a dependency, and this way the ring inherits the theme tokens like
 * everything else.
 *
 * The legend is **not** here. Desktop draws it as a table with counts and
 * percentages and the phone as a stacked list, so the caller composes it and
 * the ring stays one shape.
 *
 * A total of zero renders the empty track — nothing is drawn as if it were a
 * full ring.
 */
export function Donut({
  segments,
  label,
  size = 150,
  thickness = 22,
  hovered = null,
  onHover,
}: {
  segments: readonly DonutSegment[];
  /** What a screen reader is told: the ring itself carries no text. */
  label: string;
  size?: number;
  thickness?: number;
  /**
   * Which segment the pointer is on, or `null`. Held by the caller because the
   * legend it marks is the caller's — the ring stays one shape, and either side
   * can raise the highlight.
   *
   * Hovering is a **convenience only**: the legend beside the ring prints every
   * figure already, so nothing is reachable by pointer alone. That is why the
   * segments take no focus of their own — they would duplicate the legend in
   * the accessibility tree without adding a fact to it.
   */
  hovered?: number | null;
  onHover?: (index: number | null) => void;
}) {
  const centre = size / 2;
  const radius = centre - thickness / 2;
  const circumference = 2 * Math.PI * radius;
  const total = segments.reduce((sum, segment) => sum + segment.value, 0);

  let consumed = 0;
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      class="donut"
      role="img"
      aria-label={label}
    >
      <circle
        cx={centre}
        cy={centre}
        r={radius}
        fill="none"
        stroke="var(--track)"
        stroke-width={thickness}
      />
      {total > 0 &&
        segments.map((segment, index) => {
          const length = (segment.value / total) * circumference;
          const offset = -consumed;
          consumed += length;
          return (
            <circle
              key={segment.label}
              class="donut-seg"
              cx={centre}
              cy={centre}
              r={radius}
              fill="none"
              stroke={segment.colour}
              stroke-width={thickness}
              stroke-dasharray={`${length} ${circumference - length}`}
              stroke-dashoffset={offset}
              transform={`rotate(-90 ${centre} ${centre})`}
              // The hovered slice keeps its colour and the rest fade, exactly
              // as the chart dims its non-hovered bars. Default hit-testing is
              // what makes this work: only the painted dash answers, so each
              // point on the ring belongs to the one segment drawn there.
              opacity={hovered === null || hovered === index ? 1 : DIM_OPACITY}
              onPointerEnter={() => onHover?.(index)}
              onPointerLeave={() => onHover?.(null)}
            />
          );
        })}
    </svg>
  );
}
