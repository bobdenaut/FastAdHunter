# P2-07 — Memory Accounting

**Phase:** 2 · **Depends on:** — · **Model:** Sonnet

> Not HTTP work, like `p2-00`: the `p2-` prefix is a scheduling position. Placed
> immediately before `p2-08` because Phase 2's soak is the first consumer — a
> soak that can only report RSS answers "is it growing?" but never "growing
> *where*?".

## Goal

Every bounded structure reports its own heap, so `RSS − Σ(components)` is a
small, stable **residual**. A leak then shows as a growing residual while every
named component stays flat — a far sharper signal than watching RSS.

## Context

Measured on the RB5009 after 10 h of household traffic on 0.2.4:

```text
RSS               43,700,224 B   41.68 MiB
  ruleset heap   −23,002,595 B   21.94 MiB   (reported today)
  cache bytes    − 1,053,072 B    1.00 MiB   (reported today)
                 ─────────────
  unexplained     19,644,557 B   18.73 MiB   (45 % of RSS)
```

That 18.73 MiB is legitimate — binary pages, stacks, the tokio runtime, stats
and history buffers, musl fragmentation — but none of it is *accounted*, so it
is also exactly where a slow leak would hide unseen.

The same soak showed RSS growth decaying (+0.72 MiB/h over the first 3 h,
+0.07 MiB/h over the last 4), which is warm-up, not a leak. But 10 h at
0.81 QPS is only ~30 k queries: **a leak of a few bytes per query stays inside
the noise at that volume.** Flat RSS excludes a fast leak, not a slow one. This
task builds the instrument that can.

Hard rule 4 is "bounded everything". If everything is bounded, everything should
be able to say how big it is — and the ones that grow with *uniqueness* rather
than traffic are already capped and tested
([`ClientRegistry`](../../../crates/fah-stats/src/client_registry.rs) at 4,096
with LRU eviction; `top_n` at `HOURLY_TRACKED_DOMAINS = 256`). Their sizes are
simply not reported.

## Scope

- **Per-component heap accounting.** Add or expose a `heap_bytes()` on each
  bounded structure:
  - `fah-rules`: `Matcher::heap_bytes()` — exists.
  - `fah-dns`: cache — exists as the tracked `bytes` total.
  - `fah-stats`: aggregates + `ClientRegistry` + `top_n` counters.
  - `fah-stats::history`: the resident perf series and rollup buffers.
  - `fah-stats::query_log`: the **in-RAM** ring buffer and pending batch only.
- **Track the large single-mutation-point structures; walk the small intricate
  ones.** *(Amended twice during implementation: the original said "maintain
  running totals, never sum on read"; a first pass over-corrected to walking
  everything. Measurement settled it — see `docs/code-review/p2-07-review.md` §5.)*
  Measured, `Stats::heap()` cost 80 µs walking everything and 43 µs once the
  query ring kept a running total: the ring is 16,384 entries and has exactly
  one mutation point, so tracking is both the big win and the safe one. `top_n`
  (≤256 keys/slot) and `ClientRegistry` (≤4,096) keep being walked — their
  eviction is multi-step, which is where a running total drifts, and their cost
  is tens of µs. The cache already tracked, because it needs `bytes` for
  eviction decisions rather than reporting. Any running total needs a test
  asserting it equals a full walk after eviction.
- **Each `heap_bytes()` documents what it excludes.** Precedent for why:
  `p1-02-review.md` §4 found `heap_bytes` counting `Arc` control blocks but not
  their payload strings — an undercount in the very number feeding a budget
  claim. An unstated exclusion silently becomes residual.
- **Metrics** (`fah-metrics`, existing `fastadhunter_` prefix for consistency):
  - `fastadhunter_memory_component_bytes{component="ruleset|cache|stats|history|query_log"}`
  - `fastadhunter_memory_residual_bytes`
  - Export the residual directly rather than leaving it to a PromQL expression,
    so it is visible in a single scrape and computed from one consistent read.
- **Sample all components and RSS at the same instant.** If RSS is read at
  *t* and the components at *t+ε*, the residual carries the difference as
  noise. Snapshot under one pass on the existing telemetry poll (10 s) — this
  is not hot-path work and must not touch the query path.
- `/debug/memory` reports the same breakdown as JSON, so a human can read it
  without Prometheus.
- CONFIGURATION.md / API.md updated if the debug shape changes; ARCHITECTURE.md
  if a new port is introduced.

## Acceptance criteria

- **The residual is never negative.** A negative residual means a component
  double-counts or over-reports; assert it in a test with a populated cache,
  ruleset and stats, and log at `warn` if it ever happens in production.
- Residual is a *small* fraction of RSS and **stable over a soak window** —
  record the absolute number before and after a multi-hour run. Reducing the
  current 45 % materially is the point; state where it lands and what remains
  unaccounted (binary pages and allocator fragmentation legitimately do).
- Component sum tracks reality: filling the cache to its `max_bytes` moves
  `component_bytes{component="cache"}` by the expected amount and leaves the
  residual flat.
- **No hot-path cost.** Accounting is read on the telemetry poll only; a bench
  or allocation-counter test shows the query path is unchanged.
- Every `heap_bytes()` has a doc comment naming its exclusions.
- Gates green.

## Out of scope

Fixing any leak this finds — that is a follow-up with evidence attached.
Disk accounting for the query log and history (`retention_max_mb` already
bounds that, and CONFIGURATION.md documents the overshoot). Swapping the
global allocator for `dhat`-style profiling: it is a build-level change,
unavailable on the musl static target in production, and gives a one-off
capture where this gives a permanent signal.

## Suggested prompt

> Read plan/wip/phase2/p2-07-memory-accounting.md, ARCHITECTURE.md
> (fah-stats/fah-metrics) and ADR-0002. Add per-component `heap_bytes()` with
> bounded walks on the poll — never on the query path — export
> `fastadhunter_memory_component_bytes` and `fastadhunter_memory_residual_bytes`
> sampled in one pass on the telemetry poll, mirror the breakdown in
> `/debug/memory`, and prove the residual is non-negative, small and stable.
