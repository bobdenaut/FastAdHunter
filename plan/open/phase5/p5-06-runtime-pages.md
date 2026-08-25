# P5-06 — Runtime Pages

**Phase:** 5 · **Depends on:** p5-05 · **Model:** Opus

## Goal

Cache, Performance and Upstreams. Three pages Pi-hole has no equivalent for,
built in the same visual language as everything else.

## Context

These are where FastAdHunter's own capabilities surface: cache lifetime stages
and the two bounds, per-stage latency percentiles, and endpoint health under the
adaptive upstream strategy. Detailed in
[information-architecture.md](../../../docs/dashboard/information-architecture.md);
sketched as `Cache`, `Performance`, `Upstreams`.

## Scope

**Cache** — the cache endpoint plus the SWR and cleanup counters from telemetry.

- Entries by lifetime stage as a stacked bar, with each stage's meaning stated:
  fresh answers directly, stale answers only after a failed forward, expired is
  dead weight awaiting eviction.
- Both bounds side by side, entries and bytes, with the higher load marked as
  the one about to evict.
- Lifetime counters and hit rate.
- The clean action, with the stale purge as an explicit unchecked toggle worded
  as giving up serve-stale insurance. The result panel reports removed counts,
  before and after, freed bytes, and states that RSS does not fall by the freed
  amount because a clean never shrinks the table slab.
- The byte figure is a coarse per-entry estimate excluding hash-table slabs; it
  and the memory page are answering different questions, not disagreeing.
- SWR and background-cleanup panels.

**Performance** — the persisted per-sample series, requesting only the fields
each chart draws.

- Latency percentiles per stage, p50 and p99, for block, cache hit and forward.
  Never an average: an average hides the tail the budget is written against.
- QPS and per-interval verdict deltas.
- Percentiles are bucket-granularity estimates that saturate at the top finite
  bucket — good for a trend line, not exact quantiles, and the page says so.
- Budget lines drawn as dashed targets, never as walls.
- Decimation surfaced as on the Dashboard.

**Upstreams** — the telemetry upstream block plus health.

- Per endpoint: address, protocol, family, state, attempts, failures,
  consecutive failures, TLS handshakes, penalties, penalized seconds, probes and
  probe successes, and the closed-run failure histogram.
- Consecutive failures is the live figure; the totals are cumulative since
  process start, and the page distinguishes them.
- A degraded health status is explained in place rather than alarmed: under the
  adaptive strategy it means no endpoint is currently healthy, under fallback
  that every endpoint carries a non-zero consecutive-failure count. Neither is
  down. The page names the strategy in force so the reading is never ambiguous.
- A panel stating why no share-of-traffic chart exists: per-query upstream
  attribution is deliberately not carried.

**Mobile.** The Upstreams row becomes one card per endpoint with the counter
grid wrapping to two columns. Cache bars stay full width. Performance charts
keep their legend below the plot rather than beside it, and remain readable
without pinch-zoom.

## Acceptance criteria

- Every stage, bound and counter traces to a documented field.
- The clean action's stale toggle defaults off and is worded as a choice.
- Latency is only ever shown per stage and as percentiles.
- Degraded renders as amber with an explanation, not as a failure.
- No chart implies an enforced limit.
- Correct in both themes at all three breakpoints, verified at 390 px.
- Gates green, cargo and frontend. Bundle size recorded.

## Out of scope

Settings and Diagnostics (p5-07). Memory lives on the Diagnostics page, not
here.

## Suggested prompt

> Read docs/dashboard/information-architecture.md sections Cache, Performance
> and Upstreams, API.md, PERFORMANCE.md section Budgets, and
> plan/wip/phase5/p5-06-runtime-pages.md. Build the three runtime pages,
> keeping every budget a marker rather than a wall and every latency figure
> per-stage.
