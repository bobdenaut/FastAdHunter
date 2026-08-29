/**
 * The numeric and colour helpers both chart families need, in one place rather
 * than one copy each. Nothing here imports uPlot, so a page may read them
 * without pulling the chart chunk.
 */

const NICE_STEPS = [1, 1.5, 2, 2.5, 3, 4, 5, 7.5, 10];

/**
 * A round upper bound so five evenly spaced gridlines land on figures a reader
 * can use: 13,169 becomes 15,000, which is what `Main.dc.html` draws.
 */
export function niceMax(max: number): number {
  if (!Number.isFinite(max) || max <= 0) return 4;
  const magnitude = 10 ** Math.floor(Math.log10(max));
  const ratio = max / magnitude;
  const step = NICE_STEPS.find((candidate) => ratio <= candidate) ?? 10;
  return step * magnitude;
}

/** Five gridlines, as the artboards draw. */
export function ySplits(max: number): number[] {
  const top = niceMax(max);
  return [0, top / 4, top / 2, (top * 3) / 4, top];
}

/** `#rrggbb` to `rgba(...)`. A token that is not hex is returned unchanged
 *  rather than mangled — a dimmed bar is worth less than a drawn one. */
export function withAlpha(colour: string, alpha: number): string {
  const hex = colour.trim();
  if (!/^#[0-9a-f]{6}$/i.test(hex)) return hex;
  const value = Number.parseInt(hex.slice(1), 16);
  const r = (value >> 16) & 0xff;
  const g = (value >> 8) & 0xff;
  const b = value & 0xff;
  return `rgba(${String(r)}, ${String(g)}, ${String(b)}, ${String(alpha)})`;
}
