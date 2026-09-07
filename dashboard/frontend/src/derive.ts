import type {
  ListStatus,
  PerfItem,
  PerfLatency,
  UpstreamState,
} from './api/types';

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

/** Named `rest`, not `other`: the API's own type set carries a literal `OTHER`
 *  label for record types outside the tracked ten, and a fold spelled `other`
 *  put two different meanings a case-fold apart in one legend. */
export const REST_LABEL = 'rest';

/**
 * R13 / R14 — the donut's slices. `per_type` is summed over the active range's
 * items first (zero buckets are omitted from the response, so an absent label
 * is a zero rather than an unknown), then everything outside the `keep` largest
 * is folded into one `rest` slice, as the artboards draw.
 *
 * `rest` is appended only when something actually falls outside, and it is
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
    label: REST_LABEL,
    value: sumOver(tail, (slice) => slice.value),
  });
  return head;
}

/**
 * The type labels [`queryTypeSlices`] folded away, so the card can name what is
 * inside its `rest` slice rather than print a word the reader has to guess at.
 *
 * Derived from the two ends rather than returned by the fold: which labels fall
 * outside depends on the range, and a legend that hardcodes today's seven is
 * wrong the first time the mix changes. Empty when nothing was folded.
 */
export function foldedLabels(
  totals: Record<string, number>,
  slices: readonly Slice[],
): string[] {
  const shown = new Set(slices.map((slice) => slice.label));
  return Object.entries(totals)
    .filter(([label, value]) => value > 0 && !shown.has(label))
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([label]) => label);
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

/**
 * Which endpoint is answering queries — the first one reading `healthy`, in
 * configured order.
 *
 * That is the pool's own rule, not an inference from the counters: `adaptive`
 * walks the configured order and takes the first healthy endpoint
 * (`crates/fah-dns/src/upstream/health.rs`, `select`). So the answer moves on
 * its own — an endpoint that earns a penalty hands the role to the next one
 * until a probe brings it back.
 *
 * **`null` under any other mode.** Under `fallback` every row publishes
 * `state: healthy` because no health state exists to report, so "the first
 * healthy one" would name index 0 whatever is happening to it; and `unknown`
 * is not knowing which rule applies at all. A page that cannot read the
 * strategy cannot claim which endpoint serves.
 */
export function servingIndex(
  upstreams: readonly { state: UpstreamState }[],
  mode: UpstreamMode,
): number | null {
  if (mode !== 'adaptive') return null;
  const index = upstreams.findIndex((upstream) => upstream.state === 'healthy');
  return index === -1 ? null : index;
}

/* ─────────────────────────────────────────── p5-09 · Diagnostics · Health ── */

/**
 * D3 — how many endpoints are in each state, from `telemetry.upstreams[]`.
 *
 * **Read only under `adaptive`.** The caller gates on `upstreamMode`: under
 * `fallback` every row publishes `state: healthy` because no health state
 * exists to report, and counting those would state the opposite of the truth.
 * The three keys are the whole vocabulary — `UpstreamState` has no fourth
 * value, so a "recovering" count has nothing behind it.
 */
export function upstreamStateCounts(
  upstreams: readonly { state: UpstreamState }[],
): Record<UpstreamState, number> {
  const counts: Record<UpstreamState, number> = {
    healthy: 0,
    penalized: 0,
    probing: 0,
  };
  // A state outside the vocabulary is ignored rather than counted. `counts[x]`
  // on an unmodelled key is `undefined`, so `+= 1` wrote `NaN` **and** added
  // the key — invisible, because the rendered line reads only the three known
  // states, which is exactly why the "invents no fourth state" test passed.
  for (const upstream of upstreams) {
    if (upstream.state in counts) counts[upstream.state] += 1;
  }
  return counts;
}

/**
 * D4 — the rule lists that need attention: `failed` (the fetch broke) and
 * `rejected` (the content gate refused the body).
 *
 * `degraded` is deliberately **not** among them here: it means the fetch
 * succeeded and most of the body failed to parse, which the Lists page reports
 * with the tier breakdown that makes it readable. Neither of the two counted
 * here is an outage — a failed refresh keeps the previous copy serving.
 */
export function listsNeedingAttention<T extends { last_status: ListStatus }>(
  items: readonly T[],
): T[] {
  return items.filter(
    (item) => item.last_status === 'failed' || item.last_status === 'rejected',
  );
}

/* ─────────────────────────────────────────── p5-09 · Diagnostics · Memory ── */

/**
 * D5 — the two stats structures as one slice. They are drawn together on the
 * artboard because they are one subject: the 24 h aggregates and the bounded
 * per-client records are both what `/stats` answers from.
 */
export function statsBytes(components: {
  stats_aggregates_bytes: number;
  stats_clients_bytes: number;
}): number {
  return components.stats_aggregates_bytes + components.stats_clients_bytes;
}

export interface Extent {
  min: number;
  max: number;
}

/** D7 / D11 — the window's own extremes. `null` when the window holds no
 *  reading, which is an absence rather than a zero. */
export function extentOf<T>(
  items: readonly T[],
  of: (item: T) => number | undefined,
): Extent | null {
  let min: number | null = null;
  let max: number | null = null;
  for (const item of items) {
    const value = of(item);
    if (value === undefined) continue;
    min = min === null ? value : Math.min(min, value);
    max = max === null ? value : Math.max(max, value);
  }
  return min === null || max === null ? null : { min, max };
}

/**
 * D12 — the minor-fault **rate**, from the latest adjacent pair.
 *
 * The counter is cumulative since process start, so charting it draws a ramp
 * and says nothing; its derivative is the purge-thrash detector. A negative
 * delta is a restart rather than a negative rate, and answers `null`.
 */
export function faultRate(
  previous: { ts: string; minor_page_faults?: number },
  next: { ts: string; minor_page_faults?: number },
): number | null {
  const before = previous.minor_page_faults;
  const after = next.minor_page_faults;
  if (before === undefined || after === undefined || after < before) return null;
  const seconds = (Date.parse(next.ts) - Date.parse(previous.ts)) / 1000;
  if (!Number.isFinite(seconds) || seconds <= 0) return null;
  return (after - before) / seconds;
}

/**
 * D13 — the stacked bands, bottom-up: ruleset, cache, stats, residual.
 *
 * Each series is the running total up to and including its own band, which is
 * what an area chart needs to render a stack. **The top edge is `rss_bytes` by
 * the server-side identity** `accounted_bytes + residual_bytes = process_rss`,
 * so it is never re-derived here — a row with no `memory` block contributes a
 * gap rather than a zero column.
 */
export function stackedMemory(
  items: readonly {
    rss_bytes?: number;
    memory?: {
      ruleset_bytes: number;
      cache_estimated_bytes: number;
      stats_aggregates_bytes: number;
      stats_clients_bytes: number;
    };
  }[],
): Array<Array<number | null>> {
  const bands: Array<Array<number | null>> = [[], [], [], []];
  for (const item of items) {
    const memory = item.memory;
    if (memory === undefined || item.rss_bytes === undefined) {
      for (const band of bands) band.push(null);
      continue;
    }
    const ruleset = memory.ruleset_bytes;
    const cache = ruleset + memory.cache_estimated_bytes;
    const stats = cache + statsBytes(memory);
    // **An over-accounted row draws no components, and keeps its RSS.**
    // `fah-model`'s `over_accounted()` names the state: components claiming
    // more than RSS is an accounting bug, never a real reading. Stacked, it
    // inverts — the top band falls below the one under it — which reads as a
    // component shrinking rather than as the bug it is, so the three component
    // bands take the gap a row with no `memory` block takes.
    //
    // **RSS itself stays**, because it is not the figure in doubt: it is read
    // from `/proc/self/status` and is what the components failed to add up to.
    // Dropping it would break the one series the page's RSS state is walked
    // from, and the KPI card walks the same readings straight off the row — so
    // a gap here and no gap there is the card and the line disagreeing about a
    // reading, which is exactly what the single state walk exists to prevent.
    if (stats > item.rss_bytes) {
      bands[0]?.push(null);
      bands[1]?.push(null);
      bands[2]?.push(null);
      bands[3]?.push(item.rss_bytes);
      continue;
    }
    bands[0]?.push(ruleset);
    bands[1]?.push(cache);
    bands[2]?.push(stats);
    // The top band is RSS itself, which is the identity rather than a sum.
    bands[3]?.push(item.rss_bytes);
  }
  return bands;
}

/** What a window's shape says, once it is long enough to have one. */
export type WindowTrend = 'rising' | 'falling' | 'flat';

/**
 * D14 — the shape of a window, as a word.
 *
 * **A statement about the window, never about its current value.** It compares
 * the mean of the first third against the mean of the last third and answers
 * `rising` or `falling` only when the move clears `tolerance` of the opening
 * level, so allocator jitter on a flat series does not read as a trend.
 *
 * `null` is "this window is too short to have a shape", which is a different
 * answer from `flat` and must stay one: the residual verdict and the fault rate
 * both have to say *not enough history* rather than claim steadiness they have
 * not observed. Six is the floor — three per third, so neither mean is a single
 * reading.
 */
export function windowTrend(
  values: readonly number[],
  tolerance: number,
): WindowTrend | null {
  if (values.length < 6) return null;
  const third = Math.floor(values.length / 3);
  const mean = (slice: readonly number[]) =>
    slice.reduce((sum, value) => sum + value, 0) / slice.length;
  const first = mean(values.slice(0, third));
  const last = mean(values.slice(-third));
  if (last > first * (1 + tolerance)) return 'rising';
  if (last < first * (1 - tolerance)) return 'falling';
  return 'flat';
}

/**
 * The rows a new process starts at, read off a `peak_rss` series.
 *
 * `peak_rss` is `getrusage`'s high-water mark and is monotone within one
 * process lifetime, so a fall is a restart and never a reclaim. Each index
 * returned is the **first row of the new process**, not the last of the old.
 *
 * Two kinds of row carry no peak and are skipped rather than read as a drop:
 * a nullish one, and a `0`, which means the row predates the field or
 * `getrusage` was unavailable. Reading either as a fall would invent a restart.
 *
 * **This is the single restart model.** The trend chart's marker and the
 * residual verdict both resolve here, so the chart cannot draw a restart the
 * verdict ignores.
 */
export function restartIndices(
  peaks: ArrayLike<number | null | undefined>,
): number[] {
  const out: number[] = [];
  let previous: number | null = null;
  for (let index = 0; index < peaks.length; index += 1) {
    const value = peaks[index];
    if (value === null || value === undefined || value === 0) continue;
    if (previous !== null && value < previous) out.push(index);
    previous = value;
  }
  return out;
}

/**
 * The tail of a series that belongs to the newest process.
 *
 * **A window spanning a restart describes no process.** `windowTrend` answers a
 * question about one lifetime — residual that rises and never comes back — so
 * rows from two binaries make the answer meaningless while leaving it exactly
 * as confident. A 7 d window over a device redeployed twice that week read
 * `rising` off three lifetimes stitched together.
 *
 * Rows before the last restart are dropped, never spliced. A series with no
 * restart is returned whole, which is the ordinary case.
 */
export function sinceLastRestart<T>(
  rows: readonly T[],
  peakOf: (row: T) => number | null | undefined,
): readonly T[] {
  const restarts = restartIndices(rows.map(peakOf));
  if (restarts.length === 0) return rows;
  const start = restarts[restarts.length - 1];
  return start === undefined ? rows : rows.slice(start);
}
