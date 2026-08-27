import { request } from './core';
import type { HistoryPerf, HistoryResolution, HistorySummary } from './types';

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

export const HISTORY_PERF_PATH = '/api/v1/history/perf';

/**
 * The response keys this application asks for, and the complete list of them.
 *
 * `fields` trims the payload, not the read, and an unknown name is a server
 * `400` by design (`crates/fah-api/src/routes.rs` `parse_perf_fields`) — so the
 * names are a `const` list rather than free strings, and a typo is a type
 * error here instead of a rejected request in a browser.
 *
 * Nothing memory-shaped is requested: `rss_bytes`, `peak_rss`, `memory` and
 * `minor_page_faults` belong to the Diagnostics page, and `cache`, `upstreams`
 * and `answers_delta` are served live by endpoints the other runtime pages
 * already read.
 */
export const PERF_FIELDS = [
  'qps',
  'queries_delta',
  'blocked_delta',
  'allowed_delta',
  'latency',
] as const;

export type PerfField = (typeof PERF_FIELDS)[number];

export interface HistoryPerfQuery {
  /** RFC 3339. The window is half-open, `[from, to)`. */
  from: string;
  /** Comma-joined into `fields`. Omitting it takes the whole row, which is
   *  several times the payload this page draws. */
  fields: readonly PerfField[];
}

/**
 * `to` is never sent — the server's now is the honest end of the window — and
 * neither is `max_points`, so the perf default of 1000 stands and `stride > 1`
 * is real at the wider ranges.
 */
export function historyPerfQuery(query: HistoryPerfQuery): string {
  const params = new URLSearchParams();
  params.set('from', query.from);
  params.set('fields', query.fields.join(','));
  return params.toString();
}

/** One request per range selection. Nothing on the Performance page polls. */
export function getHistoryPerf(
  query: HistoryPerfQuery,
  signal?: AbortSignal,
): Promise<HistoryPerf> {
  return request<HistoryPerf>(
    `${HISTORY_PERF_PATH}?${historyPerfQuery(query)}`,
    { ...(signal === undefined ? {} : { signal }) },
  );
}
