import { request } from './core';
import type { HistoryResolution, HistorySummary } from './types';

export const HISTORY_SUMMARY_PATH = '/api/v1/history/summary';

export interface HistorySummaryQuery {
  /** RFC 3339. The window is half-open, `[from, to)`. */
  from: string;
  /** Omitted defaults to now, which is what every range on the Dashboard
   *  wants — sending a client clock as `to` would only add skew. */
  to?: string;
  resolution?: HistoryResolution;
  /** Omitted takes the server default of 5000, which makes `stride > 1`
   *  unreachable at the three offered ranges. */
  max_points?: number;
}

export function historySummaryQuery(query: HistorySummaryQuery): string {
  const params = new URLSearchParams();
  params.set('from', query.from);
  if (query.to !== undefined) params.set('to', query.to);
  if (query.resolution !== undefined) params.set('resolution', query.resolution);
  if (query.max_points !== undefined) {
    params.set('max_points', String(query.max_points));
  }
  return params.toString();
}

/** A range query, not a poll: it is refetched when the operator changes the
 *  range and at no other time. */
export function getHistorySummary(
  query: HistorySummaryQuery,
  signal?: AbortSignal,
): Promise<HistorySummary> {
  return request<HistorySummary>(
    `${HISTORY_SUMMARY_PATH}?${historySummaryQuery(query)}`,
    { ...(signal === undefined ? {} : { signal }) },
  );
}
