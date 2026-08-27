import type { PerfItem, PerfLatency } from './api/types';

/**
 * **The derived display values of the shipped pages live here**, keyed to the
 * tables that authorise them: `R*` is p5-06's §8.2 (Dashboard, Lists) and `E*`
 * is p5-08's §6 (Cache, Performance, Upstreams).
 *
 * Phase 5 standing constraint 1 forbids figures the API does not support. A
 * derivation is a stated arithmetic function of documented fields; anything not
 * in this module is read from a response verbatim, so a reviewer checks one
 * file against those tables rather than hunting arithmetic through the pages.
 *
 * *(p5-06 R5, R7, R11, R12, R15, R16 and R17 are counted, printed or laid out
 * where they are rendered — one operator or a `length` each; the p5-06 review's
 * closure pass records that and why wrapping them adds no checkable place.)*
 *
 * Never derived at all, and deliberately absent: a 24 h HTTP figure (the API
 * has none), a combined DNS + HTTP "queries" total (the pipelines are never
 * summed), an upstream share of traffic (no per-query attribution exists), and
 * a per-client blocked figure from `/stats` (it is not there).
 */

/** R2 / R3 — the chart's range totals, summed from the same items that draw
 *  the bars. `/stats` is a rolling 24 h window and cannot state a 7 d total. */
export function sumOver<T>(items: readonly T[], of: (item: T) => number): number {
  let total = 0;
  for (const item of items) total += of(item);
  return total;
}

/** R4 — the one aggregate the API does not serve. Exact over one response, not
 *  an average of averages. The per-bucket `blocked_percent` is served and is
 *  read verbatim by the tooltip; this is the range figure, which has no field. */
export function blockedPercent(queries: number, blocked: number): number {
  return queries === 0 ? 0 : (blocked / queries) * 100;
}

/** R8 / R9 / R18 — max-normalisation over the rendered rows, which is the
 *  scale `Main.dc.html` settles with its own figures (3,140 / 4,021 = 78 %).
 *  An all-zero column draws empty tracks rather than full ones. */
export function shareOfMax(value: number, max: number): number {
  return max <= 0 ? 0 : value / max;
}

export interface Slice {
  label: string;
  value: number;
}

export const OTHER_LABEL = 'other';

/**
 * R13 / R14 — the donut's slices. `per_type` is summed over the active range's
 * items first (zero buckets are omitted from the response, so an absent label
 * is a zero rather than an unknown), then everything outside the `keep` largest
 * is folded into one `other` slice, as the artboards draw.
 *
 * `other` is appended only when something actually falls outside, and it is
 * never one label renamed — a single leftover label keeps its own name.
 */
export function queryTypeSlices(
  totals: Record<string, number>,
  keep = 4,
): Slice[] {
  const ranked = Object.entries(totals)
    .filter(([, value]) => value > 0)
    .map(([label, value]) => ({ label, value }))
    .sort((a, b) => b.value - a.value || a.label.localeCompare(b.label));

  if (ranked.length <= keep + 1) return ranked;

  const head = ranked.slice(0, keep);
  const tail = ranked.slice(keep);
  head.push({
    label: OTHER_LABEL,
    value: sumOver(tail, (slice) => slice.value),
  });
  return head;
}

/** R13 — a slice as a percentage of the summed `per_type`. Here rather than in
 *  the card so §8.2 really is checkable against this one module. `0` on an
 *  empty range: an all-zero donut draws its empty track, not five NaNs. */
export function sliceShare(value: number, total: number): number {
  return total <= 0 ? 0 : (value / total) * 100;
}

/** Sums `per_type` across a range's items. Kept beside the fold because the two
 *  are one figure in two steps and splitting them invites a second summation. */
export function sumPerType(
  items: ReadonlyArray<{ per_type: Record<string, number> }>,
): Record<string, number> {
  const totals: Record<string, number> = {};
  for (const item of items) {
    for (const [label, value] of Object.entries(item.per_type)) {
      totals[label] = (totals[label] ?? 0) + value;
    }
  }
  return totals;
}

/**
 * R18 / R19 — the Upstream Health bar, and **the complete list of upstream
 * arithmetic on this page**.
 *
 * ```text
 * bar width  = attempts / max(attempts)      relative workload
 * overlay    = failures / attempts           failure rate within that bar
 * state      → the dot and its colour, independently
 * text       → "N attempts · M failures", verbatim, always visible
 * ```
 *
 * Nothing else about an upstream is computed here: no success rate, no health
 * score, no share of traffic, no availability percentage, and nothing derived
 * from `consecutive_failures`, `failure_runs`, `penalty_round`, `penalties`,
 * `penalized_seconds_total`, `probes`, `probe_successes` or `tls_handshakes`.
 * Those fields exist on the response and this card does not render them.
 *
 * **The overlay is never given a minimum width.** At 12 failures in 201,883 it
 * is 0.006 % of the bar and simply disappears — the figure survives in the
 * printed text. Widening it to be visible would draw a failure rate the
 * endpoint does not have, which is the one thing this card must not do.
 */
export interface UpstreamBar {
  /** 0..1 of the track. */
  width: number;
  /** 0..1 **of the bar**, not of the track. */
  overlay: number;
}

export function upstreamBar(
  attempts: number,
  failures: number,
  maxAttempts: number,
): UpstreamBar {
  return {
    width: shareOfMax(attempts, maxAttempts),
    overlay: attempts <= 0 ? 0 : Math.min(1, failures / attempts),
  };
}

/* ------------------------------------------------------- p5-08 runtime pages */

/**
 * **The runtime pages' half of the table above.** `E*` rows are the p5-08
 * plan's §6; the same property holds — one module against one table, and
 * nothing arithmetic on Cache, Performance or Upstreams lives anywhere else.
 *
 * Never derived at all on those three pages, and deliberately absent here: any
 * latency **average** (an average hides the tail the budget is written
 * against), any upstream share of traffic, success rate, availability
 * percentage or health score, anything computed from `tls_handshakes`, any
 * delta of two telemetry reads, any delta of `last_duration_micros` (it is a
 * last-value gauge), and any figure combining `/stats` with these endpoints.
 */

/** E1 — the cache stage bar's fourth band. Same rule as R7 on the Dashboard:
 *  `capacity` can round slightly below the configured maximum, and a negative
 *  band is not drawn. */
export function freeEntries(capacity: number, entries: number): number {
  return Math.max(0, capacity - entries);
}

/** E3 — `hits + misses`, which is resolved queries (`pass + allow`): a blocked
 *  query never reaches the cache, so it is in neither term. */
export function cacheLookups(hits: number, misses: number): number {
  return hits + misses;
}

/** E4 — the hit-rate donut. A zero denominator draws the empty track rather
 *  than dividing. */
export function cacheHitRate(hits: number, misses: number): number {
  const lookups = cacheLookups(hits, misses);
  return lookups === 0 ? 0 : (hits / lookups) * 100;
}

/** Which of the two bounds evicts first. The bars themselves render
 *  `load_percent` and `byte_load_percent` verbatim; only this callout is
 *  derived. */
export type CacheBound = 'entries' | 'bytes' | 'equal';

/** E5 — eviction runs until entries **and** bytes are each back inside their
 *  bound, so the higher load is the one that triggers first (API.md §Cache). */
export function closestBound(
  loadPercent: number,
  byteLoadPercent: number,
): CacheBound {
  if (loadPercent > byteLoadPercent) return 'entries';
  if (byteLoadPercent > loadPercent) return 'bytes';
  return 'equal';
}

/**
 * E10 / E12 — one latency percentile, seconds to milliseconds, with the
 * no-traffic case mapped out.
 *
 * `LatencySummary` reports exactly `0.0` for a stage with **no queries in that
 * interval**, and a real reading is a bucket upper bound, so it can never be
 * exactly `0.0`. Plotting the zeros would draw latency dips the engine never
 * had; `null` is a gap in the series and an em-dash on a tile.
 */
export function latencyMs(seconds: number): number | null {
  return seconds === 0 ? null : seconds * 1000;
}

/**
 * E10's other half — **which** row a tile reads, beside `qpsStats`'s E14, which
 * scopes the same way. Both pick over the **served** rows only: decimation drops
 * whole rows, so at `stride > 1` the last row served is not the last row the
 * recorder wrote, and the tile labels say so.
 *
 * Rows without a `latency` block are skipped rather than read as zeros — a
 * trimmed `fields` drops the key entirely (absent, not null), and an absent key
 * is not a measurement of nothing.
 */
export function latestLatency(items: readonly PerfItem[]): PerfLatency | null {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const latency = items[index]?.latency;
    if (latency !== undefined) return latency;
  }
  return null;
}

/** E11 — how much of its budget a stage is using, and whether it has reached
 *  it. Capped for display only; `over` is the one documented boundary, and
 *  there is no tier between zero and the budget to invent. */
export interface BudgetProximity {
  /** 0..100. */
  percent: number;
  over: boolean;
}

export function budgetProximity(
  valueMs: number,
  budgetMs: number,
): BudgetProximity {
  if (budgetMs <= 0) return { percent: 0, over: false };
  const ratio = (valueMs / budgetMs) * 100;
  return { percent: Math.min(100, Math.max(0, ratio)), over: ratio >= 100 };
}

/**
 * E14 — the QPS stat row, over the **served** rows only.
 *
 * At `stride > 1` the response is a 1-in-stride subsample, so `busiest` is the
 * busiest sample that was served rather than the range's true peak, and
 * `latest` lags real time by up to `stride × interval`. The labels say so; this
 * function only picks.
 */
export interface QpsStats {
  latest: number | null;
  busiest: number | null;
}

export function qpsStats(values: readonly (number | undefined)[]): QpsStats {
  let latest: number | null = null;
  let busiest: number | null = null;
  for (const value of values) {
    if (value === undefined) continue;
    latest = value;
    if (busiest === null || value > busiest) busiest = value;
  }
  return { latest, busiest };
}

/**
 * E16 — the `pass` band, the derivation `crates/fah-model/src/perf.rs`
 * documents. Floored at zero: a restart boundary inside the range zeroes the
 * cumulative counters the server deltas against, so one sample can come back
 * with the parts exceeding the whole.
 *
 * `allowed_delta` is the **real `allow` verdict**, never the derived
 * `permitted` band — the two are different figures and never share a word.
 */
export function passDelta(
  queriesDelta: number,
  blockedDelta: number,
  allowedDelta: number,
): number {
  return Math.max(0, queriesDelta - blockedDelta - allowedDelta);
}

/** E22 — the failure-run histogram, normalised to the row's **own** largest
 *  bucket so a quiet endpoint is not drawn as a busy one. All zero draws four
 *  empty tracks; the counts are printed verbatim beneath either way. */
export function failureRunShares(runs: readonly number[]): number[] {
  const max = runs.reduce((highest, run) => Math.max(highest, run), 0);
  return runs.map((run) => shareOfMax(run, max));
}

/**
 * KTD5 / E25 — the strategy decides whether the health block can be read at
 * all.
 *
 * Under `fallback` every row publishes `state: healthy`, `penalty_round: 0` and
 * zeros for penalties, penalized seconds, probes and probe successes, which
 * API.md states means *no health state exists to report* — not "everything is
 * fine". Those cells are therefore omitted rather than rendered as good news.
 * `unknown` is `GET /config` having failed: the counters are still verbatim,
 * and the page says it cannot name the strategy.
 */
export type UpstreamMode = 'adaptive' | 'fallback' | 'unknown';

export function upstreamMode(strategy: string | null | undefined): UpstreamMode {
  if (strategy === 'adaptive') return 'adaptive';
  if (strategy === 'fallback') return 'fallback';
  return 'unknown';
}
