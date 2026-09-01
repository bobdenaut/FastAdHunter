# Review — P5-11 Per-Endpoint Upstream Round-Trip Time

## Implementation Summary

Implemented [p5-11-upstream-rtt.md](../../../plan/wip/phase5/p5-11-upstream-rtt.md)
as scoped, §§1–5. §6 (API.md, CONTEXT.md) is proposed below and **not done** —
Working agreement 1.

### What was implemented

- **§1 — shared primitive.** `AtomicHistogram<N>` plus the workspace's single
  `quantile` and `saturating_delta` in
  `crates/fah-common/src/histogram.rs` (new). `fah-metrics`' `Histogram` and
  `StageHistogram::quantile`/`delta` are now thin wrappers; `BUCKETS_SECONDS`
  changed type `&[f64]` → `[f64; 11]` to feed the const-generic primitive.
  Quantile semantics moved verbatim (1-based `ceil(q·count).max(1)` target,
  `0.0` when empty, top-bound saturation). `fah-metrics` tests pass
  unmodified; a grep for a second bucket-position loop finds only fah-common.
- **§2 — model.** `UpstreamRtt` (count, sum_seconds, p50, p99, serde-skipped
  cumulative buckets) + pinned `UPSTREAM_RTT_BUCKETS_SECONDS` (1 ms–2 s, 11)
  in `fah-model/src/perf.rs`; `UpstreamSample.rtt` with `#[serde(default)]`,
  `Eq` dropped for `PartialEq`.
- **§3 — measurement.** Per-server histogram in
  `fah-dns/src/upstream/mod.rs`, observed on success only at both call sites
  (fallback loop, adaptive path — probes included); `status()` snapshots
  count/sum/cumulative. No quantile math in fah-dns.
- **§4 — wiring.** `main.rs`: `lifetime_rtt` before `set_upstreams`
  (telemetry poll), `interval_rtt` in `build_perf_sample` (address-matched
  delta via the shared functions, boot sample reads the lifetime).
- **§5 — dashboard.** Endpoint-row RTT band (p50/p99/mean/answers timed,
  `—` for absent/zero); new `rtt-chart.tsx` + `use-rtt-history.ts` over
  `/history/perf?fields=upstreams` (existing field), newest-row endpoint set,
  4-series cap, `spanGaps` off, recording-off empty state, single-flight
  `/config` reader; `UPSTREAM_PERF_FIELDS` kept disjoint from the other two
  field lists; CSS band with its own selector; mobile 2-column fold.

### Design decisions

- Interval delta iterates `saturating_delta` into a fixed array — the one
  quantile/delta pair serves `Vec<u64>` (StageHistogram) and `[u64; 11]`
  (UpstreamRtt) without conversion allocations.
- Wire rows carry 4 numbers; buckets never serialize. `prev` is always the
  in-memory `MetricsSnapshot`, never a deserialized row (plan §4).
- History-row semantics: `count`/`sum_seconds` cumulative, `p50`/`p99`
  per-interval; `/telemetry` percentiles are lifetime. Stated in the TS types
  and destined for API.md (§6).

### Tests and gates

- New: fah-common primitive semantics (8); fah-dns pool-level
  `rtt_counts_only_answered_attempts` (acceptance 4); main.rs
  `upstream_rtt_tests` — lifetime, interval-vs-lifetime, idle interval,
  reorder, replaced endpoint, boot (6); frontend — 4 row-cell tests, 4
  `endpointOrder` tests, field-list disjointness (951 total, was 942).
- Seven `UpstreamSample` fixture files updated (compiler-forced).
- Gates: `cargo fmt --check`, clippy `-D warnings`,
  `cargo test --workspace`, `tsc --noEmit`, frontend 951/951,
  `npm run build` — 131 391 B gzip of 153 600 (85.5 %), brotli 116 512 B.

### Deviations from the plan

- File map said `fah-dns/Cargo.toml` gains `fah-common` — it already had it;
  `fah-metrics/Cargo.toml` is the one that gained the dependency. No layering
  change either way (L3 → L1).
- Plan §5 said `—` for the cells "when `rtt` is absent … or `count == 0`".
  Intentional clarification (owner-approved, review finding 2): with a present
  `rtt` block and `count == 0`, the **answers timed** cell prints `0` — the
  count is exact and "zero answers timed" is a real reading — while
  `p50`/`p99`/`mean` keep `—`. An absent block prints `—` in all four cells.

### Owner approvals — resolved 2026-08-31

- §6 doc edits **approved and applied**: API.md — `rtt` in both sample rows
  plus a `/telemetry` reading bullet and a `/history/perf` interval-exception
  paragraph; CONTEXT.md — term *Upstream RTT*. Owner additionally directed a
  PERFORMANCE.md note: instrumentation cost (~58 ns dev box, ~0.5 µs at 9×,
  no budget — network time) pointing at §Measurements below.
- Phase table row added to `plan/wip/phase5/CLAUDE.md` (owner-requested).

## Findings

Reviewed 2026-08-31 against the plan's scope, acceptance criteria and the root
CLAUDE.md hard rules. Diff-first (base `2384109`); every gate re-run by the
reviewer, not taken from the summary. The working tree also carries the
separately reviewed forward-stage-labels change
([dashboard-forward-stage-labels-review.md](dashboard-forward-stage-labels-review.md));
all Performance-page and PERFORMANCE.md hunks were attributed there and are not
part of this task — acceptance 9 holds for p5-11 itself.

### Minor

**1. Acceptance 4's pool test proves "failed", not specifically "timed out".**
`rtt_counts_only_answered_attempts` (`fah-dns/src/upstream/mod.rs:1020`) uses
`dead_addr()` — a bound-then-dropped loopback UDP socket. `plain::query`
connects the socket (`plain.rs:64`), so on both Linux and Windows the ICMP
port-unreachable typically surfaces as a fast recv error, not a wait until the
200 ms attempt timeout. The criterion says "a **timed-out** attempt leaves
`rtt.count` unchanged". The code is correct either way — the observe sites are
`is_ok()`-gated, so every failure class is excluded — but the evidence is one
class short. The existing silent-socket idiom (`mod.rs:953`, bound and never
answering) forces the real timeout path. Impact if unchanged: none functional;
the criterion's exact scenario rests on inspection rather than a test.
Recommend: add a silent-socket variant (cheap, non-flaky — deterministic
timeout). Fix-or-defer: owner's call; defer is defensible.

**2. "answers timed" prints `0` where the plan says `—`.** Plan §5: "`—` when
`rtt` is absent … **or `count == 0`**". `endpoint-row.tsx:111-114` renders `—`
only for an absent block; a present block with `count == 0` prints `0`.
p50/p99/mean comply (`rttLabel`/`meanLabel` guard zero). Inference, not
measurement: `0` is arguably the more honest cell — the count is exact and
"zero answers timed" is a real reading, unlike a `0 ms` percentile — but it is
a literal deviation from the plan text and is undocumented in the summary's
deviations section. Fix-or-defer: owner decides which side wins; either the
cell changes or the deviation is recorded as intended.

### Nitpick

**3. "Capped at 4 series" implemented as 4 endpoints = 8 plot series.**
`MAX_SERIES = 4` in `rtt-chart.tsx:20` caps endpoints; each contributes a
p50/p99 pair. Consistent with the plan's own "one colour per endpoint" and the
acceptance-5 wording as tested (`caps the plot at four endpoints`), so read as
the intended meaning. Recorded so the ambiguity does not resurface.

**4. `quantile`'s top-bound fallback is `unwrap_or(0.0)` where the moved code
had `unwrap()`** (`fah-common/src/histogram.rs:58`). Reachable only with empty
`bounds` and `count > 0`, which no caller can produce (both bound arrays are
non-empty consts). Semantics identical for every real input; no change asked.

### Verified — no finding

- **Acceptance 1**: grep confirms the only bucket-position `fetch_add`
  histogram is `fah-common/src/histogram.rs`; `StageHistogram::quantile` and
  `::delta` are thin calls; no quantile/delta math in `fah-dns` (`status()`
  publishes zeros for p50/p99, filled in L4).
- **Acceptance 2**: per attempt, one `Instant::now()` before `server.query`,
  `elapsed()` + 3 relaxed `fetch_add`s (bucket, count, sum) on success only; a
  failed attempt costs one unused clock read. No locks, allocations, or regex.
- **Acceptance 3**: per endpoint, 13 × `AtomicU64` (11 buckets + count + sum),
  fixed; histograms live in `UpstreamServer`, dropped with the pool on reload.
  Nothing grows with traffic or uptime.
- **Acceptance 5**: all six Rust cases present (`fah-common` 8 unit tests;
  `main.rs::upstream_rtt_tests` 6 — lifetime, interval-vs-lifetime, idle
  interval → 0.0 not a bound, address-matched reorder, replaced endpoint reads
  own lifetime, boot); all five frontend cases present in
  `upstreams.test.tsx` + `resources.test.ts`.
- **Acceptance 6**: `#[serde(skip)]` buckets deserialize to zeros and interval
  math never reads deserialized rows (`prev` is the in-memory
  `MetricsSnapshot`, `spawn_perf_sampler` `main.rs:879-903`); TS `rtt?` +
  `rttLabel`/`meanLabel` guards → `—`, `rttMs` → `null` → broken line, no
  `NaN` path found.
- **Acceptance 7**: `fah-metrics` histogram/snapshot tests untouched; the one
  `registry.rs` test edit is the compiler-forced `rtt: default()` fixture
  line, which the file map exempts.
- **Acceptance 8, re-run by reviewer**: `cargo fmt --check` clean; clippy
  `-D warnings` clean; `cargo test` on fah-common/fah-metrics/fah-model/
  fah-dns/fastadhunter all green; `tsc --noEmit` clean; frontend 951/951;
  `npm run build` 131 391 B gzip of 153 600 (85.5 %), brotli 116 512 B —
  matches the summary's figures.
- **Layering**: fah-common (L1) ← fah-metrics, fah-dns (L3) — downward only;
  `UpstreamRtt` and the bounds const are pure data in fah-model (hard rule 2);
  percentile computation sits in L4 exactly as planned.
- **Interval semantics**: persisted rows keep cumulative `count`/`sum_seconds`
  with per-interval `p50`/`p99` (verified in `interval_rtt` — the delta struct
  is local, the row keeps cumulative fields); address-matched with
  `saturating_sub` absorbing reload resets.
- **Web placement**: RTT band below the counters, above the failure-run
  histogram, not health-gated (rendered in fallback mode, test-pinned); chart
  card between endpoint cards and the No-Pie/States row; recording-off empty
  state replaces the chart alone; single-flight `/config` via
  `useConfigReader` (p5-06 F11) — the page's own mount read was converted to
  it, removing the duplicate-request path.
- **Out of scope respected**: no Prometheus exposition of the per-endpoint
  histogram, no cadence change, no Live Feed attribution, probes/setup not
  filtered out.
- **File-map deviation** (summary §Deviations) confirmed:
  `fah-dns/Cargo.toml` already carried `fah-common`; `fah-metrics/Cargo.toml`
  gained it; `fah-common/Cargo.toml` needed no change. L3 → L1 either way.

### Fixes applied — 2026-08-31, owner-approved

- **Finding 1 — FIXED.** Added
  `rtt_ignores_an_attempt_that_ran_into_the_timeout`
  (`fah-dns/src/upstream/mod.rs`): a bound-but-silent first endpoint forces
  the real 200 ms timeout path (the existing silent-socket idiom); asserts
  `failures == 1` with `rtt.count == 0`, zero buckets and zero
  `sum_seconds` on it, and `rtt.count == 1` on the answering endpoint.
  Deterministic — the timeout, not ICMP behavior, drives the failure.
- **Finding 2 — RESOLVED, no code change.** `answers timed` printing `0` for a
  present block with `count == 0` is accepted as an intentional clarification
  of plan §5 and recorded in §Deviations above. `—` stays for `p50`/`p99`/
  `mean` with no timed answers and for an absent block in all four cells.
- **Nitpicks 3 and 4** — accepted as-is by the owner (4 endpoints = up to 8
  plotted series; `unwrap_or(0.0)` unreachable with the pinned non-empty
  bucket arrays).
- Verification: `cargo fmt --check` clean; clippy `-D warnings` clean;
  `cargo test --workspace` green (both rtt pool tests pass); frontend
  untouched by the fix, so its gates stand as previously recorded.
  Performance page untouched.

### Measurements — A/B instrumentation cost, 2026-08-31 (owner-requested)

Setup. Baseline: clean worktree at `2384109` (last commit before this task).
Candidate: this working tree (p5-11 + review fixes). Device: dev box —
i9-13980HX, 32 GB, Windows 11 IoT LTSC 2024; unpinned (no `taskset` on
Windows — PERFORMANCE.md §Benchmarking hygiene caveat applies). Same commands,
same flags, runs sequential, criterion 100 samples; figures are criterion
means with CI bounds. Criterion's stored-baseline "change" lines ignored
(docs/measurement-traps.md). RB5009 conversion: measured ~9× factor.

Coverage note (what each bench can see). `full_pipeline` uses the
`InstantForwarder` mock and `upstream_select` is policy-math only — neither
path contains the new observe sites, so they serve as side-effect guards
(struct layout, `fah-metrics` wrapper delegation). The direct cost is carried
by `fah-dns --bench upstream` (real `UpstreamPool::forward` over loopback
UDP, observe sites included) and by a standalone harness against the shipped
`fah-common` primitive. `full_pipeline` needed
`RUSTFLAGS="-C debug-assertions=yes"` on **both** sides — the p5-04
`test-harness` compile guard rides `fastadhunter`'s dev-dependency edge into
every release-profile bench build of that crate; identical flags keep the A/B
fair, absolute figures carry the debug-assertions tax.

| Bench (mean) | Baseline `2384109` | Candidate | Delta |
| --- | --- | --- | --- |
| `upstream/forward_udp_answered` | 61.96 µs | 62.14 µs | +0.3 % — inside CI overlap |
| `upstream/forward_udp_answered_adaptive` | 64.14 µs | 63.28 µs | −1.3 % — inside CI overlap |
| `select/2_healthy` | 1.503 ns | 1.509 ns | noise |
| `select/8_first_7_penalized` | 8.151 ns | 8.158 ns | noise |
| `update/success` | 4.077 ns | 4.134 ns | +1.4 % — noise band |
| `transition/claim_probe` | 1.411 µs | 1.432 µs | +1.5 % — noise band |
| `full_pipeline/blocked_query` | 1.749 µs | 1.697 µs | −3.0 % |
| `full_pipeline/forwarded_query_overhead` | 2.322 µs | 2.148 µs | −7.5 % |
| `full_pipeline/sustained_throughput` | 138.89 µs / 1.382 Melem/s | 140.80 µs / 1.364 Melem/s | +1.4 % time — noise band |

`upstream/forward_udp_refused*` arms: CI spans of 5–40 ms on this host
(Windows ICMP/scheduler variance) — unusable, excluded; the observe sites are
not on the failure path anyway.

Direct cost, standalone harness (50 M iterations per case, 1 M warmup,
release + LTO, against the tree's `fah-common::AtomicHistogram` with
`UPSTREAM_RTT_BUCKETS_SECONDS`; absolute-only by design — the baseline has no
such code to A/B against):

| Case | ns/op |
| --- | --- |
| `Instant::now()` + `elapsed()` pair | 41.9 |
| `observe()`, first bucket (0.5 ms) | 10.3 |
| `observe()`, mid bucket (30 ms) | 10.5 |
| `observe()`, past top bound (full 11-compare scan) | 7.8 |
| **Full added cost: pair + observe** | **58.3** |

Reading. ~58 ns per **answered forwarded attempt**, uncontended; ~0.5 µs
converted at 9× to the RB5009 — ≤0.05 % of even a 1 ms upstream answer, and
invisible inside the 62 µs loopback forward (CIs overlap). Block and
cache-hit paths never execute it. No bench crossed the 10 % gate; the deltas
that exist sit inside run-to-run noise on an unpinned desktop. Scope of
claim: this corpus (single-endpoint loopback UDP, bench workloads above),
this device, these two trees; RB5009 figure is a factor conversion, not an
on-device measurement — the phase's Stage B / soak remains the on-device
verification.

### Post-merge finding — 2026-09-01: RTT observed lookups `attempts` ignores

Found on the running RB5009 (`0.3.1`, uptime 48 min) while verifying the
Upstreams page against live telemetry, not by re-reading the diff.

| Endpoint | attempts | rtt.count |
| --- | --- | --- |
| `1.1.1.1` | 492 | **570** |

**`rtt.count` exceeded `attempts` by 78, with `failures` 0.** Two counters
the page prints side by side, over two different populations, under a
caption asserting they are one.

Cause — `walk_adaptive`, `fah-dns/src/upstream/mod.rs`:

```rust
if recording {
    health.attempts.fetch_add(1, Ordering::Relaxed);   // gated on HealthMode
}
...
if result.is_ok() {
    server.rtt.observe(started.elapsed());              // was NOT gated
}
```

`resolve_host` forwards with `HealthMode::Ignore` so a list-URL hostname
lookup moves no health counter — deliberately, and pinned by
`resolve_host_with_one_family_black_holed_moves_no_health_state`. RTT was
outside that gate, so those lookups were timed while never counted as
attempts.

**This contradicted the shipped contract, which was already correct.**
[API.md §upstreams[].rtt](../../../API.md) reads "time-to-answer of that
endpoint's **answered** attempts only" and [CONTEXT.md §Upstream RTT](../../../CONTEXT.md)
reads "answered attempts, measured per Endpoint". The docs were right; the
code did not match them. The review above therefore describes a population
the implementation did not have.

**FIXED — owner ruling, option A of two.** `rtt.observe` is now gated on
`recording`. The rejected option B was to widen the documented population to
match the code, which would have required editing API.md *and* CONTEXT.md —
the binding vocabulary — to describe an accident. A restores conformance and
needed no doc change at all; the dashboard caption
(`upstreams/rtt-chart.tsx`, "measured over the attempts that were answered")
became true rather than needing a rewrite.

- Diff: one line in `walk_adaptive`, `if recording && result.is_ok()`.
- New test `resolve_host_records_no_rtt_under_adaptive`: ten `resolve_host`
  calls leave `attempts` **and** `rtt.count` at 0, then one `forward` moves
  both to 1 — the invariant, not the symptom.
- **Fallback deliberately unchanged.** It honours no `HealthMode` at all and
  counts `resolve_host` legs as attempts, pinned by
  `resolve_host_still_counts_attempts_under_fallback`. Attempts and RTT are
  symmetric there, so the defect cannot arise; gating RTT alone would have
  created it in the opposite direction.
- Gates: `cargo fmt --check` clean, clippy `-D warnings` clean (exit 0),
  `cargo test --all-features --workspace` green, `fah-dns --lib` 200 passed.
- **Not deployed.** `f34af6c` is running unpatched on the RB5009 for the
  `0.3.1` soak, so the live Upstreams page keeps showing the wider count
  until the next deployment.

Why the original review missed it: acceptance 4 tested that a *failed* or
*timed-out* attempt is not observed. Nothing tested that a *non-attempt* is
not observed, because `HealthMode::Ignore` was treated as a health-counter
concern and RTT was not thought of as a health counter.

### Status

**PASS** — all findings fixed or resolved; A/B shows no measurable
regression and the direct instrumentation cost is quantified above. §6 doc
edits approved and applied (API.md, CONTEXT.md, plus the owner-directed
PERFORMANCE.md note).

**Amended 2026-09-01:** PASS stands, with the post-merge finding above fixed
in the working tree and awaiting deployment. The task's own acceptance was
met; what the review did not cover was the interaction with
`HealthMode::Ignore`.
