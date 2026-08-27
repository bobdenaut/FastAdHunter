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
