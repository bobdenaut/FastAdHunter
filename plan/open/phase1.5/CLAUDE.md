# Phase 1.5 — Observability Persistence (pre-Phase 2 base)

**Objective:** persist the observability data long-term on the SSD so a future
Pi-hole-style dashboard has 30/60/90 days (REST-configurable) of everything to
draw from — query volume, block rate, query types, top domains/clients, cache
performance, upstream health, latency percentiles, and **process RAM/RSS over
time**. Plus the recorded soak follow-up: a byte-aware cache cap.

**Why this phase exists (before Phase 2):** Phase 1 shipped a working resolver
and a *live* API, but the aggregate time-series is a bounded **24-hour ring**
([../../closed/phase1](../../closed/phase1) → `fah-stats`), and the Prometheus
counters are ephemeral (reset on restart, built for an external scraper). A
dashboard needs durable, queryable history. Building that foundation now — a
"better base" — means Phase 2's UI work reads a stable data contract instead of
inventing one. The ~91h RB5009 soak (docs/code-review/p1-11-soak.md) is what
motivated this: it proved the system is bounded, and it also proved the value
of a persisted RSS/perf series (the soak did it *externally* with a curl loop;
this phase makes the router self-host it).

**Why this order:** the rollup + perf-sample *writers* first (they define the
on-disk contract), then retention config, then the read API that serves them,
then the cache cap (independent hardening), then verification last.

## What to persist (resolved with the user)

- **Cache contents → NO** (ephemeral by design, root CLAUDE.md hard rule #4).
  The cache **stats** (`GET /api/v1/cache` body:
  `entries/capacity/fresh/stale/expired/evictions/load_percent/hits/misses`)
  **→ YES**, sampled over time.
- **Prometheus raw counters → NO** — sample the *quantities* internally.
- **Query aggregates + RAM/perf → YES**, rolled up to flat JSONL on `/data`
  (ADR-0002: no embedded DB), bounded and pruned by `retention_days`.

Per-query events already persist (`/data/query_log/segments/`); their retention
is unchanged. Long-term *per-client* time-series is out of scope for the base
(recent per-client is derivable from the raw log) — revisit with the UI.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p1.5-01-history-rollups.md` | Hourly→daily aggregate rollup store in `fah-stats`; `HourRollup`/`DailyTopN` PODs; boot/load/prune | Sonnet | WAITING |
| 2 | `p1.5-02-perf-sample-series.md` | `Metrics::snapshot()` + histogram percentiles; binary sampler (RSS + cache port + upstream); `PerfSample` persisted via `fah-stats` | Opus | WAITING |
| 3 | `p1.5-03-history-retention-config.md` | `[history]` config + REST-live-settable retention (30/60/90); CONFIGURATION.md | Sonnet | WAITING |
| 4 | `p1.5-04-history-query-api.md` | `GET /api/v1/history/{summary,perf,top}`; API.md | Sonnet | WAITING |
| 5 | `p1.5-05-cache-byte-cap.md` | Byte-aware cache cap so the ceiling respects the 128 MB budget under adversarial input + sustained-throughput measurement | Opus | WAITING |
| 6 | `p1.5-06-verification.md` | Unit tests (rollup math, prune, sampler), e2e (populate→query history), on-device soak proving disk- and memory-bounded | Sonnet | WAITING |

**Definition of done:** after a day of traffic, `/data/history/` holds hourly
rollups and per-interval perf samples pruned to `retention_days`;
`GET /api/v1/history/summary|perf|top` return chart-ready series including
RSS and cache stats over time; changing `history.retention_days` via
`POST /api/v1/config` takes effect live; the sampler adds no unbounded memory
(hard rule #4) and the cache byte-ceiling respects the 128 MB budget under the
`test_aleator.py` worst case; workspace gates green.

**Key risks:** sampler must never touch the hot path (it reads snapshots on a
timer, like the existing telemetry poll) — mitigation: no per-query work, all
writes off-thread on the flush cadence; disk growth must stay bounded —
mitigation: reuse the `SegmentWriter` prune pattern, verify on-device;
histogram percentiles are estimates from fixed buckets — document the
resolution, don't imply exactness.

**Naming:** `phase1.5` sorts before `phase2` under the plan's
lowest-number-first rule; tasks use the `p1.5-NN` prefix.
