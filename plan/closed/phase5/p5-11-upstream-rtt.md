# P5-11 — Per-Endpoint Upstream Round-Trip Time

Owner-requested addendum to the runtime pages (p5-08). A first implementation
was built ad-hoc, reviewed, and **reverted**; this task exists so the second
implementation follows a plan instead of improvising. The review's findings
are folded into the scope and acceptance criteria below and are binding.

## Goal

The Upstreams page answers "how fast is each endpoint?", not only "is it
failing?". Per-endpoint round-trip time is measured in `fah-dns`, served on
`/telemetry` and `/history/perf`, and rendered as cells on each endpoint row
plus one range chart — without duplicating the histogram/quantile machinery
that already exists in `fah-metrics`.

## Context

- The only latency measured today is the aggregate `forward` stage
  (`fah-metrics/src/registry.rs:38` — end-to-end, all endpoints pooled, RFC
  8767 failed first attempts included). No per-endpoint attribution exists
  anywhere; this task creates the measurement.
- `GET /history/perf?fields=upstreams` already exists
  (`fah-api/src/wire.rs:460`) — no new route, no new field-filter work.
- Layering (root CLAUDE.md hard rule 1): `fah-dns` and `fah-metrics` are L3
  siblings and may not import each other. Any shared code lives in L1.
- The reverted implementation was functionally correct and fully tested; its
  one MEDIUM defect was three copies of histogram/quantile logic
  (review finding 1). This plan removes that defect by construction.

## Scope

### 1. Histogram primitive moves to `fah-common` (L1)

- Extract from `fah-metrics/src/histogram.rs` + `snapshot.rs` into
  `fah-common`: a fixed-bucket atomic histogram parametrized over its
  bucket-bound array (relaxed atomics, no locks, no allocation per
  observation), a cumulative snapshot, and **one** pure quantile function and
  **one** pure cumulative-delta function.
- The pure functions take slices, so both callers fit without conversion:
  `StageHistogram` holds `Vec<u64>`, `UpstreamRtt` holds `[u64; 11]` —
  `quantile(bounds: &[f64], cumulative: &[u64], count: u64, q: f64) -> f64`
  and `delta(cur: &[u64], prev: &[u64]) -> …` (elementwise
  `saturating_sub`).
- **Move the quantile, do not rewrite it.** Its semantics are
  `StageHistogram::quantile`'s, exactly: target is the 1-based
  `ceil(q · count).max(1)`-th observation; result is the smallest bound whose
  cumulative count reaches it; `0.0` when `count == 0`; saturates at the last
  finite bound when the target falls in the implicit `+Inf` bucket.
- `fah-metrics` re-exports/wraps the primitive; `StageHistogram::quantile` and
  `::delta` become thin calls into it. **No behavior change** — every existing
  `fah-metrics` test passes unmodified.
- After this task exactly one quantile implementation exists in the workspace.

### 2. Model type in `fah-model`

- `UpstreamRtt` (data only, hard rule 2): `count: u64`, `sum_seconds: f64`,
  `p50: f64`, `p99: f64`, `#[serde(skip)] buckets` (cumulative). Bucket bounds
  const `UPSTREAM_RTT_BUCKETS_SECONDS`, exactly:

  ```rust
  [0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0]
  ```

  Wider than the stage buckets on both ends because this is network time, not
  engine time; the 2 s top bound is what the chart footnote names.
- `UpstreamSample` gains `#[serde(default)] rtt: UpstreamRtt` (drops `Eq`,
  keeps `PartialEq`). Old persisted rows deserialize to the default; old
  engines beside a new dashboard are handled in §5.

### 3. Instrumentation in `fah-dns::UpstreamPool`

- One histogram per `UpstreamServer`, observed around `server.query` on
  **success only** — a timed-out or failed attempt is never observed, so a
  dead endpoint cannot pin p99 at `timeout_ms`. Both call sites: the fallback
  loop and the adaptive path (probes included — a probe is a real round trip).
- `status()` includes the snapshot. No quantile math in `fah-dns`.
- Measured value is time-to-answer: UDP retransmit legs and cold TCP/TLS
  connection setup are inside it. That is stated in the docs (§6), not
  filtered out.

### 4. Wiring in `crates/fastadhunter/main.rs` (L4)

- Telemetry poll: lifetime p50/p99 via the shared quantile before
  `set_upstreams`.
- Perf sampler: per-interval p50/p99 — delta the cumulative buckets against
  the previous in-memory snapshot, **matched by address** (a reload reorders
  or replaces endpoints; `saturating_sub` absorbs counter resets), via the
  shared delta + quantile. First sample after boot reads the lifetime.
- Persisted/`/history/perf` row semantics: `count`/`sum_seconds` cumulative,
  `p50`/`p99` per-interval. Interval math never reads deserialized rows (the
  skipped buckets exist only in memory).

### 5. Dashboard

- Endpoint row: an RTT group (own selector, not the counter grid) with four
  cells — p50, p99, mean (`sum/count`), answers timed. `—` when `rtt` is
  absent (old engine, field optional in TS) or `count == 0`; an exact `0.0`
  percentile renders as `—`, never as `0 ms`.
- Range chart on the Upstreams page: p50 (dashed) / p99 (solid) per endpoint
  over `/history/perf?fields=upstreams`, reusing the existing range-chart
  idiom (`useRecordedRange`, ranges, decimation footer, recording-off empty
  state, single-flight `/config` reader — p5-06 F11). Endpoint set from the
  **newest** row carrying endpoints; capped at 4 series; `spanGaps` off so an
  interval with no answers breaks the line. No budget marker — this is
  network time.
- Footnote states: answered attempts only; bucket-granularity estimates
  saturating at 2 s; this is the non-FastAdHunter part of the Performance
  page's `forward` stage.

### 6. Documentation (propose, then wait — Working agreement 1)

- API.md: `rtt` block on `/telemetry` and `/history/perf`, with the
  interval-vs-lifetime percentile semantics stated.
- CONTEXT.md: term *upstream RTT* — time-to-answer of answered attempts,
  probes included, timeouts excluded.
- These are proposed as concrete edits after the code is done; listing here is
  not permission.

## Web placement

Everything lands on the **Upstreams** page (sidebar → Upstreams). No other
page changes: Performance keeps the aggregate `forward` stage, Live Feed keeps
per-query rows with no upstream attribution.

Page order after this task, top to bottom:

1. Existing header and endpoint cards — each card gains the RTT group as a
   full-width band **below the attempt/failure counters, above the
   failure-run histogram**: four cells (`p50`, `p99`, `mean`, `answers
   timed`) with the caption "round trip, answered attempts only". Live
   figures, read from `/telemetry` like the counters beside them; shown in
   every strategy mode (fallback rows too — RTT is not health-gated).
2. **New card: "Round-trip time by endpoint, p50 and p99"** — the range
   chart, inserted between the endpoint cards and the No-Pie/States row.
   Same range chips as the Dashboard and Performance charts (default 24h),
   solid p99 / dashed p50, one colour per endpoint with a legend. When
   `history.enabled` is off, this card alone is replaced by the
   recording-off empty state; the endpoint cards above are live and stay.

Cells print milliseconds (`millisLabel`); the chart's y-axis is ms like the
Performance latency chart.

## File map

Where each scope unit lands. New files marked **(new)**; test-fixture updates
(`UpstreamSample` literals across crates) are compiler-forced and not listed.

| Unit | Files |
| ---- | ----- |
| §1 primitive | `crates/fah-common/src/histogram.rs` **(new)**; `fah-metrics/src/histogram.rs` + `snapshot.rs` become wrappers; `fah-common/Cargo.toml` |
| §2 model | `crates/fah-model/src/perf.rs` (`UpstreamRtt`, bounds const, `UpstreamSample.rtt`); `fah-model/src/lib.rs` re-exports |
| §3 measurement | `crates/fah-dns/src/upstream/mod.rs` (per-server histogram field, two observe sites, `status()`); `fah-dns/Cargo.toml` gains `fah-common` |
| §4 wiring | `crates/fastadhunter/src/main.rs` (telemetry poll: lifetime percentiles; `build_perf_sample`: address-matched interval delta) |
| §5 row cells | `dashboard/frontend/src/pages/upstreams/endpoint-row.tsx`; `api/types.ts` (`UpstreamRtt`, optional `Upstream.rtt`, `PerfItem.upstreams?`); `styles/components.css` (own selector, not the counter grid) |
| §5 chart | `dashboard/frontend/src/pages/upstreams/rtt-chart.tsx` **(new)**; `pages/upstreams/use-rtt-history.ts` **(new)**; `pages/upstreams.tsx` (mount + recording-off state); `api/history.ts` (`UPSTREAM_PERF_FIELDS = ['upstreams']`, disjoint from the other field lists) |
| Tests | `fah-common` histogram/quantile units; `fah-dns` pool-level timeout test; `main.rs` interval/lifetime units; `pages/upstreams.test.tsx`; `api/resources.test.ts` (field-list disjointness) |

## Acceptance criteria

1. One quantile and one delta implementation in the workspace; `fah-dns`
   contains no copy of either; grep for a second `fetch_add`-histogram
   confirms only the `fah-common` primitive.
2. Hot path per forwarded attempt: ≤1 `Instant::now()` pair + relaxed atomic
   RMWs on success; no locks, no allocations, no regex (hard rule 3).
3. Memory is fixed per configured endpoint (11×8 B buckets + 2 counters);
   nothing grows with traffic or uptime (hard rule 4).
4. A timed-out attempt leaves `rtt.count` unchanged — proven by a pool-level
   Rust test, not only by UI copy (review finding 7).
5. Unit tests cover: empty histogram → 0.0; quantile = smallest bound reaching
   target; saturation at top bound; interval vs lifetime; address-matched
   reorder; boot sample. Frontend tests cover: cells render, absent/zero →
   `—`, counter-grid count unchanged, newest-row endpoint order, 4-series cap.
6. Old history rows (no `rtt`) and an old engine beside a new dashboard both
   render `—` / an empty chart — no `NaN`, no crash.
7. `fah-metrics` behavior unchanged: its existing tests pass without edits.
8. Gates green: fmt, clippy `-D warnings`, `cargo test --workspace`,
   `npm run typecheck`, `npm run build` (bundle stays under the 150 KB gzip
   gate), frontend tests.
9. Performance page is untouched by this task.

## Out of scope

- An engine-overhead timer for the `forward` stage (the missing counterpart of
  PERFORMANCE.md's `< 1 ms` row) — separate decision, separate task.
- Per-query upstream attribution in Live Feed.
- Excluding probes or connection setup from the measurement.
- Any PERFORMANCE.md budget change; any change to `TELEMETRY_POLL` or the
  sampler cadence (note: a `history.sample_interval_seconds` below the 10 s
  telemetry poll would yield empty intervals — document, don't fix).
- Prometheus exposition of the per-endpoint histogram.

## Suggested prompt

> Implement plan/wip/phase5/p5-11-upstream-rtt.md exactly as scoped — the
> scope and acceptance criteria are binding. Do not add scope, do not edit
> .md docs without asking, and stop after the review handoff per
> plan/wip/phase5/CLAUDE.md.
