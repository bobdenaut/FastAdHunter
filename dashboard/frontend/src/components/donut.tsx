export interface DonutSegment {
  label: string;
  value: number;
  colour: string;
}

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
}: {
  segments: readonly DonutSegment[];
  /** What a screen reader is told: the ring itself carries no text. */
  label: string;
  size?: number;
  thickness?: number;
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
        segments.map((segment) => {
          const length = (segment.value / total) * circumference;
          const offset = -consumed;
          consumed += length;
          return (
            <circle
              key={segment.label}
              cx={centre}
              cy={centre}
              r={radius}
              fill="none"
              stroke={segment.colour}
              stroke-width={thickness}
              stroke-dasharray={`${length} ${circumference - length}`}
              stroke-dashoffset={offset}
              transform={`rotate(-90 ${centre} ${centre})`}
            />
          );
        })}
    </svg>
  );
}
