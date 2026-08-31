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
 * Nothing memory-shaped is requested here: `rss_bytes`, `peak_rss`, `memory`
 * and `minor_page_faults` are the Diagnostics · Memory page's own list below,
 * `upstreams` is the Upstreams page's own list below that, and `cache` and
 * `answers_delta` are served live by endpoints the other runtime pages already
 * read.
 */
export const PERF_FIELDS = [
  'qps',
  'queries_delta',
  'blocked_delta',
  'allowed_delta',
  'latency',
] as const;

/**
 * What Diagnostics · Memory asks for, and the complete list of it. Two pinned
 * lists rather than one union used loosely: each page's request is asserted
 * against its own constant in `resources.test.ts`, so widening one cannot
 * quietly widen the other's payload.
 *
 * `rss_bytes` is the stack's total and the rail's window maximum, `peak_rss`
 * the dashed series beside it and the restart-boundary signal, `memory` the
 * four bands, `minor_page_faults` the fault-rate derivative.
 */
export const MEMORY_PERF_FIELDS = [
  'rss_bytes',
  'peak_rss',
  'memory',
  'minor_page_faults',
] as const;

/**
 * What Upstreams asks for, and the complete list of it. `/telemetry` already
 * carries the endpoints live, so the only thing this range read adds is the
 * one figure on the row that is per-interval rather than cumulative: the
 * round-trip percentiles. A whole row is several times the payload for the
 * two series the chart draws.
 */
export const UPSTREAM_PERF_FIELDS = ['upstreams'] as const;

export type PerfField =
  | (typeof PERF_FIELDS)[number]
  | (typeof MEMORY_PERF_FIELDS)[number]
  | (typeof UPSTREAM_PERF_FIELDS)[number];

export interface HistoryPerfQuery {
  /** RFC 3339. The window is half-open, `[from, to)`. */
  from: string;
  /** Comma-joined into `fields`. Omitting it takes the whole row, which is
   *  several times the payload either page draws. */
  fields: readonly PerfField[];
  /**
   * Optional. Omit it and the endpoint's default of 1000 stands, which is what
   * Performance wants — its charts are shaped by the range, not by the sample.
   *
   * **Memory sends it, because for that page the default silently lies.** 1000
   * against a 24 h window of 60 s samples decimates to a stride of 2, so a
   * chart captioned "every sample" would be drawing every other minute.
   * Capped at 5000 server-side; a larger request is rejected, not clamped.
   */
  maxPoints?: number;
}

/**
 * `to` is never sent — the server's now is the honest end of the window.
 */
export function historyPerfQuery(query: HistoryPerfQuery): string {
  const params = new URLSearchParams();
  params.set('from', query.from);
  params.set('fields', query.fields.join(','));
  if (query.maxPoints !== undefined) {
    params.set('max_points', String(query.maxPoints));
  }
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
