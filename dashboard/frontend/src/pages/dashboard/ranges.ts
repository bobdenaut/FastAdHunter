import type { HistoryResolution } from '../../api/types';

/**
 * The three ranges the chips offer, and the query each one issues.
 *
 * **7 d and 30 d ask for `resolution=day`.** 168 hourly bars is unreadable at
 * any width, so a week is seven bars and a month is thirty. `to` is omitted:
 * the server's now is the honest end of the window, and sending a browser clock
 * would only add skew.
 */
export type RangeKey = '24h' | '7d' | '30d';

export const RANGE_KEYS: readonly RangeKey[] = ['24h', '7d', '30d'];

interface RangeSpec {
  label: string;
  /** How far back the window starts, in milliseconds. */
  spanMs: number;
  resolution: HistoryResolution;
}

const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;

export const RANGES: Record<RangeKey, RangeSpec> = {
  '24h': { label: '24 h', spanMs: 24 * HOUR_MS, resolution: 'hour' },
  '7d': { label: '7 d', spanMs: 7 * DAY_MS, resolution: 'day' },
  '30d': { label: '30 d', spanMs: 30 * DAY_MS, resolution: 'day' },
};

/**
 * The resolution the **plotted** data is in, which is not always the one the
 * chips are asking for.
 *
 * A range change swaps the request immediately and the response lands a round
 * trip later, and the chart deliberately keeps the previous range's bars up
 * meanwhile rather than blanking. Formatting those bars with the new range's
 * axis therefore labelled thirty daily buckets `00:00 · 00:00 · 00:00 · 00:00`
 * for the length of the fetch. Reading the resolution off the response instead
 * keeps the axis and the bars describing the same data at every instant; both
 * flip together when the response arrives.
 *
 * The requested range is the fallback for the one case with nothing plotted —
 * the first load, where `summary` is still `null`.
 */
export function plottedResolution(
  summary: { resolution: HistoryResolution } | null,
  range: RangeKey,
): HistoryResolution {
  return summary?.resolution ?? RANGES[range].resolution;
}

export function rangeQuery(
  key: RangeKey,
  now: number,
): { from: string; resolution: HistoryResolution } {
  const spec = RANGES[key];
  return {
    from: new Date(now - spec.spanMs).toISOString(),
    resolution: spec.resolution,
  };
}
