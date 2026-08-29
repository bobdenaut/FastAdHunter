export interface Segment {
  label: string;
  value: number;
  colour: string;
}

/**
 * A segmented proportion bar with the legend beside it — cache fresh/stale/
 * expired, the per-list rule partition. The legend carries each segment's word
 * and figure, so the colours are a second signal rather than the only one.
 *
 * A total of zero renders as the empty track: nothing is drawn as if it were a
 * full band.
 */
export function StageBar({
  segments,
  format = (value: number) => value.toLocaleString(),
}: {
  segments: readonly Segment[];
  format?: (value: number) => string;
}) {
  const total = segments.reduce((sum, segment) => sum + segment.value, 0);
  return (
    <div>
      <div class="seg">
        {total > 0 &&
          segments.map((segment) => (
            <div
              key={segment.label}
              style={{ flex: segment.value, background: segment.colour }}
            />
          ))}
      </div>
      <div class="seg-legend">
        {segments.map((segment) => (
          <span key={segment.label}>
            <span class="sw" style={{ background: segment.colour }} />
            {segment.label} <span class="mono">{format(segment.value)}</span>
          </span>
        ))}
      </div>
    </div>
  );
}
