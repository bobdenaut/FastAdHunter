# p2-07 — Historical Memory Breakdown

**Task:** [plan/wip/phase2/p2-07-historical-memory-breakdown.md](../../plan/wip/phase2/p2-07-historical-memory-breakdown.md)
**Gates:** `fmt --check` clean · `clippy --workspace --all-targets -D warnings` clean · `test --workspace` **483 passed, 0 failed** (twice consecutively)

---

## 1. What it does

`RSS − Σ(components)` is now a reported **residual**. A leak shows as *residual
growing while the named components stay flat*, because the growth you
legitimately expect has been subtracted out. Growth in a component is not a
leak — it is that structure filling toward its cap.

Before: 41.68 MiB RSS with only the ruleset (21.94) and cache (1.00) accounted
— **45 % unexplained**, which is exactly where a slow leak would hide.

New surfaces:

- `fastadhunter_memory_component_bytes{component="ruleset|cache|stats_aggregates|stats_clients|query_log_ring|query_log_pending"}`
- `fastadhunter_memory_residual_bytes`
- `GET /api/v1/debug/memory` gains the same breakdown plus `accounted_bytes`
  and `residual_bytes` (API.md updated).

## 2. Layering — `fah-dns` was not touched

Raised during review: *"don't put metrics logic in DNS."* Correct, and it
required no change there at all. The cache already tracks `bytes` for
**eviction**, and it crosses to the API through the existing `CacheSource`
port. Each crate reports only its own size; the binary is the one layer allowed
to see all of them:

```text
fah-rules   matcher.heap_bytes()      ─┐
fah-dns     cache.stats().bytes        ├─→  binary (L4)  ─→ fah-metrics
fah-stats   stats.heap()              ─┘      one pass, one instant
```

`fah-metrics` never learns what a cache or a ring is — it receives a finished
`MemoryBreakdown`, the same way `RulesetSnapshot` already worked.

## 3. Deduplication — corrected mid-implementation

Also raised during review: *"be careful not to introduce code duplication."*
Two real duplications had crept in and were removed:

1. **The `accounted` sum and the residual formula each existed twice** —
   once in `fah-metrics`, once hand-rolled in `routes.rs`. Adding a seventh
   component would have updated one surface and silently under-reported on the
   other.
2. **Three near-identical structs** — `fah_stats::StatsHeap`,
   `fah_api::StatsHeapView`, and `fah_metrics::MemorySnapshot`.

All collapsed onto **one type in `fah-model` (L1)**:
[`MemoryBreakdown`](../../crates/fah-model/src/memory.rs) with `StatsHeap`,
`accounted()`, `residual()` and `over_accounted()` defined exactly once. The
precedent is `PerfSample` / `CacheStatsSample`, which already live there for
the same reason — a shape four crates need, where none may import another.

Consequences: `fah_metrics::MemorySnapshot` deleted, `fah_api::StatsHeapView`
deleted, and the binary's adapter became a **pass-through** with nothing to
translate. Net effect of the dedup was *less* code than the first version, not
more.

## 4. Correctness details

**Underflow guard** (raised in review). `residual()` uses `saturating_sub`, and
a negative residual — impossible in reality, so always an accounting bug — is
reported separately by `over_accounted()` rather than hidden by the floor. The
binary logs that condition at `warn`. A wrong instrument is worse than no
instrument. Three tests cover it, including the over-accounting case.

**One-pass sampling.** The whole breakdown, RSS included, is gathered in a
single pass on the 10 s telemetry poll. Reading RSS at a different instant from
the components would push the skew into the residual — destroying the signal.
`/debug/memory` reads live instead, so the two can differ by up to one interval;
API.md says so.

**No async locks.** `Stats::heap()` takes only the three sync mutexes, never
the `segment` / `history` / `perf` writers, so a poll can never contend with a
flush doing I/O.

**Every `heap_bytes()` names its exclusions** — the struct's own inline size
when it lives inside another counted structure, allocator fragmentation (which
belongs in the residual), and anything behind a shared `Arc` counted at its
owner. `p1-02-review.md` §4 had already caught one of these counting `Arc`
control blocks but not their payloads.

## 5. Accounting cost — measured, then halved

Raised in review: *"is this a permanent feature, and what does iterating 16k
elements cost?"* Fair, so it was measured rather than argued
(`crates/fah-stats/tests/heap_cost.rs`, `#[ignore]`d — a wall clock is not a
gate):

| | `Stats::heap()`, saturated |
| --- | ---: |
| Walking everything | **80 µs** (x86) |
| Ring tracking incrementally | **43 µs** (x86) |

The ring was ~half the cost — 16,384 entries each chasing a pointer to a
heap-allocated domain. It now maintains a running total instead, updated at its
single mutation point, guarded by `tracked_bytes_match_a_full_walk_across_eviction`
(drift after eviction is exactly the bug a running total introduces).

Estimated 200–300 µs on the RB5009's 1.4 GHz core, every 10 s: **~0.003 % duty
cycle**, and at 0.81 QPS the odds of a query landing on the held lock are
roughly one every 12 hours — well inside the <1 ms p99 budget even then.

The remaining walks stay walks, deliberately. `top_n` (≤256 keys/hour slot) and
`ClientRegistry` (≤4,096) both have multi-step eviction — min-count, and LRU
with a named/unnamed preference — which is where a running total would drift.
Tens of µs is not worth that risk. **The right rule was never "always track" or
"always walk": it is track where mutation is single-point and the structure is
large, walk where eviction is intricate and the structure is small.**

## 6. A constraint I reversed, deliberately

The task said *"maintain running totals; never sum on read."* I first
implemented **bounded walks** everywhere and amended the task to match. §5
shows that was over-corrected: measurement put half the cost in one structure,
and that structure was the easy one to track.

Final position, and the task file now says this: **track where the structure is
large and has a single mutation point; walk where eviction is intricate and the
structure is small.** The ring tracks. The bounded counters walk. The cache
already tracked, because it needs `bytes` for eviction decisions rather than for
reporting — a distinction the original blanket constraint blurred in the other
direction.

Worth recording that neither the original rule nor my reversal of it was right,
and only measuring settled it.

## 7. Tests

`heap_accounting_tracks_recorded_traffic_and_stays_bounded` asserts both halves:
the instrument *moves* when memory moves (aggregates and clients grow with 500
distinct domains/clients), and it *stops* moving once the caps are reached
(40,000 further distinct keys grow the total by <1.25×).

That test failed on its first run at **9.8×**, which was my error, not the
code's: I compared a near-empty registry against a saturated one, so legitimate
fill toward the 4,096 and 16,384 caps read as unbounded growth. The fix was to
compare two *saturated* states — which is what hard rule 4 actually promises.
Worth recording, because the same mistake would misread a soak: growth toward a
cap is not a leak.

## 8. Unrelated find: the e2e gate was flaking ~1 run in 4

While running the gates repeatedly, `the_binary_blocks_resolves_reports_and_reconfigures_live`
failed intermittently. **Pre-existing and unrelated to p2-07** — `e2e.rs` is
untouched by this change — but worth fixing, because a flaky gate makes every
future "gates green" claim unreliable.

Not a port *conflict*. Windows `WSAEACCES` (os error 10013):

```text
binding TCP 127.0.0.1:53560: An attempt was made to access a socket in a way
forbidden by its access permissions. (os error 10013)
```

Hyper-V / WinNAT reserves whole blocks of ephemeral ports (`netsh interface
ipv4 show excludedportrange protocol=tcp`). Binding inside one fails with
"forbidden by access permissions", **not** "address already in use" — so the
harness's `is_port_conflict` did not match it, the retry never fired, and a
randomly-chosen reserved port failed the whole suite.

Fix: `is_port_conflict` now also recognises `10013` / "forbidden by its access
permissions" as a retryable port problem. Verified with 8 consecutive runs of
the e2e test and 3 consecutive full-workspace runs, all green.

## 9. Limits

- **The component figures are estimates.** `HashMap` does not expose its true
  allocation, so bucket cost is modelled at hashbrown's 8/7 sizing rounded to a
  power of two. Absolute values carry that error; the *trend*, which is what
  the residual is for, does not.
- **The residual is still large** (~18.7 MiB of 41.68 measured before this
  landed) and legitimately so — binary pages, stacks, tokio, musl retention.
  Shrinking it further would mean accounting the runtime itself, which is not
  worth it. What matters is that it is now *visible and trendable*.
- **Not yet deployed.** Deploying restarts the container and would destroy the
  running soak's continuity — the very soak this instrument exists to inform.
  Deploy after the T+24h read; the *next* soak gets full accounting.
- A leak of a few bytes per query is still not excluded by existing data. The
  instrument is what will exclude it, over a full soak.

---

## 10. Addendum — the accounting now reports its own cost (temporary)

**Gates:** `fmt --check` clean · `clippy --workspace --all-targets -D warnings`
clean · `test --workspace` **484 passed, 0 failed**

§5's numbers are x86 numbers, and the ARM figure in this document (200–300 µs)
is an extrapolation. Worse, the x86 measurement **structurally could not include
the RSS read** — `process_rss()` returns `None` on Windows, so the
`/proc/self/status` open+read+parse that the RB5009 actually pays every 10 s was
never in the 43 µs. So the number is now measured on the target:

```text
fastadhunter_memory_collection_seconds
```

Wall time of the **whole pass** — `rules.matcher()`, `cache_stats()`,
`stats.heap()`, `matcher.heap_bytes()` and the RSS read — not of `stats.heap()`
alone, because the whole pass is what the 10 s tick costs. Captured before the
`over_accounted()` `warn!`, so a logging call can never inflate it.

**Deliberately kept out of `fah_model::MemoryBreakdown`.** It is metadata about
the measurement, not a memory figure, and that type is permanent while this
instrument is not — a `u64` there would also make `PartialEq` time-dependent for
a struct that otherwise compares two states of memory. It lives as one
`AtomicU64` in `fah_metrics::Metrics` instead: **two crates touched**
(`fah-metrics`, `fastadhunter`), against five for the shared-type version, and
removal is the same four hunks in reverse. §3's dedup lesson does not apply —
there is one producer and one surface, so there is nothing to duplicate.

Reported in seconds per Prometheus base-unit convention (matching
`fastadhunter_ruleset_compile_duration_seconds`) from a microsecond capture;
`memory_collection_cost_is_exported_in_seconds` pins both the conversion and the
unset-reads-as-zero case.

**Removal condition, stated so it does not become permanent by default:** delete
it once a soak shows the on-device figure stable. It is a `# TEMPORARY`-marked
gauge, an atomic + setter, a timer in the poll, and one API.md paragraph.

**Executed 2026-08-02** — removed in §11 below, exactly those four hunks. Last
on-device reading was `0.000986` (986 µs, inside the 495 µs–4.86 ms spread
already recorded), and the pass has since moved to `spawn_blocking`, so its
duration no longer reaches query latency at all.

---

## 11. The persisted series (2026-08-02) — the task's last deliverable

**Gates:** `fmt --check` clean · `clippy --workspace --all-targets -D warnings`
clean · `test --workspace` **778 passed, 0 failed** (one pre-existing
environmental failure, below)

Everything above is a *single instant*. The 0.2.9 soak's memory evidence is
therefore one `debug-memory.json` reading — 24.9 MB residual of 52.9 MB RSS,
with no way to say whether it was flat. This closes that.

### What shipped

| Change | Where |
| --- | --- |
| `MemoryComponents { ruleset, cache, stats }` split out of `MemoryBreakdown` | `fah-model/src/memory.rs` |
| `PerfSample.memory` + `PerfSample.minor_page_faults`, both `#[serde(default)]` | `fah-model/src/perf.rs` |
| `Metrics::memory()` — the sampler reads the poll's breakdown | `fah-metrics/src/registry.rs` |
| `MemoryComponentsResponse`, flattened into `MemoryResponse`, nested in a perf row | `fah-api/src/wire.rs` |
| `memory` + `minor_page_faults` in `PerfFields` / `NAMES` / `ALL` | `fah-api/src/wire.rs` |
| `fastadhunter_memory_collection_seconds` and its atomic deleted (§10) | `fah-metrics`, `fastadhunter` |

### Four decisions

1. **The row stores components only.** No `rss` (that is `rss_bytes`, already a
   `?fields=` selector and already in 30 days of files) and no residual — a
   stored derived value can disagree with its own inputs after any change to
   what a component counts. `residual_bytes` is computed on read.
2. **`rss_bytes` now comes from the breakdown, not a fresh `/proc` read.** The
   poll is 10 s and the sampler 60 s, so a fresh read would have paired a
   just-read RSS with components up to 10 s older and put the skew in the
   residual — the exact failure §4 exists to prevent. Also removes one procfs
   read per sample.
3. **One mapping, not two.** `/debug/memory` and the perf row both go through
   `MemoryComponentsResponse::of(&MemoryBreakdown)`; `MemoryResponse` `flatten`s
   it, so live/persisted agreement is structural — §3's lesson applied before
   the duplication landed rather than after.
4. **Only `minor_page_faults` from `AllocatorStats`.** Its *derivative* is the
   purge-thrash signal and a rate needs consecutive rows. The other three are
   monotone over process lifetime — as a series they are ramps.

### Cost

| | |
| --- | ---: |
| JSON per row | ~160 B |
| 30 days at `sample_interval_seconds = 60` | **6.9 MB**, pruned by `retention_days` |
| Hot path | unchanged — no `fah-dns` / `fah-http` file is in the diff |
| Per sample | one `/proc/self/status` read **removed** |

### Tests

- `a_row_written_before_the_memory_breakdown_still_deserializes` — 30 days of
  existing rows keep parsing, reading back all-zero.
- `history_perf_derives_the_residual_from_each_row` — 55,000,000 − 30,000,000 =
  25,000,000 computed on read; both new `?fields=` names trim to themselves.
- `live_and_persisted_breakdowns_use_the_same_keys_and_arithmetic` — same eight
  keys on both surfaces, `accounted_bytes` equals the sum.
- `history_e2e` — the breakdown survives disk → HTTP, carrying no second RSS.

### Not done — needs the device

Two acceptance criteria are unmet and cannot be met on a dev box: **where the
residual lands against a freshly measured mimalloc baseline**, and **its slope
over a soak window**. Both are p2-08 soak work; the 45 % figure in §1 predates
the allocator swap and must not be quoted as the number being improved on.

`fastadhunter --test e2e` fails here with the §8 `WSAEACCES` again — the whole
reserved block was drawn, not one port. Identical failure on the stashed clean
tree, so it is environmental.

---

## 12. Closed on-device (2026-08-06) — the instrument works

**The two remaining criteria are met, twice, across two independent processes.**
The series was pulled from the router's own `/history/perf` in one request, not
reconstructed from an external poll loop.

### Windows

| Window | Span | Sampling | Raw | Used |
| --- | --- | --- | ---: | ---: |
| T0 process | `2026-08-02T10:18:25Z` → `2026-08-04T23:35Z` (61.3 h) | 60 s | 3,675 | 3,668 |
| Current process | `2026-08-04T23:59:09Z` → `2026-08-06T12:11Z` (36.2 h) | 360 s | 363 | 323 |

Exclusions, identical for both: each process's first sample (`accounted_bytes =
0` — sampler runs before the ruleset registers, per the baseline) and any sample
above 90 MiB RSS (list-refresh compile transients — see §Not this task).

### Residual — the criteria

| Criterion | T0 window | Current window |
| --- | --- | --- |
| Residual on **every** sample | 4,657 / 4,657 rows carry `memory.residual_bytes` — none absent | ← same request |
| Slope, whole window | **+0.082 MiB/h** | **+0.027 MiB/h** |
| Slope, **final third** | **+0.174 MiB/h** (20.4 h, n=1,216) | **−0.057 MiB/h** (12.1 h, n=100) |
| Mean residual | 39.52 MiB | 39.68 MiB |
| Band | 25.1 – 62.7 MiB | 21.7 – 61.1 MiB |
| Residual / RSS | 57.4 % | 58.1 % |
| `events_dropped_total` | — | 0 |

**Not drift.** The final-third slopes disagree in sign, the band is ~37 MiB
wide, and the two windows' means agree to 0.4 % despite being separate processes
of different length and sampling rate. A real leak would leave the second
process starting from a lower level than the first ended at; it does not.

### Live vs persisted, same instant

| Source | Timestamp | `process_rss` |
| --- | --- | ---: |
| `/history/perf` last row | `2026-08-06T12:11:09Z` | 70,545,408 |
| `/debug/memory` live read | `2026-08-06T12:11:41Z` | 70,533,120 |

12,288 B apart (0.02 %) across a 32 s gap — both through the same `residual()`.

### Residual against a mimalloc baseline — stated, not compared

**57–58 % of RSS**, flat, at a mean of 39.5 MiB. This is the baseline figure for
0.2.10 under mimalloc. §1's 45 % is **not** the number this improves on and must
not be quoted as one: it was measured pre-mimalloc, on a 21.94 MiB ruleset
against today's 27.03 MiB, and with only two components accounted. What remains
unaccounted is what §9 said it would be — binary text pages (13.1 MiB image),
thread stacks, tokio, and allocator slack. None of it grows with traffic or
uptime.

### The criterion that failed

**"Exactly one container start in the window"** — five starts on 2026-08-04
between 23:36:05 and 23:59:09Z, visible in the series as `accounted_bytes = 0,
cache_entries = 0, minor_page_faults = 0` rows. Cause is **external to
FastAdHunter**: an upstream IPv6 outage at the ISP, diagnosed by the repo owner
by rebooting and reconfiguring the router. `auto-restart-interval=none`, so none
of the five was FAH restarting itself.

The 61.3 h pre-reboot window satisfies every other criterion **on its own**, and
the 36.2 h post-reboot window independently reproduces it. Two config values
also changed at that boundary — `history.sample_interval_seconds` 60 → 360 and
`query_log` persistence off (RouterOS was reporting ~700 MB of disk) — which is
why the second window is 6× coarser.

### Not this task

The window also recorded a **compile-transient peak RSS of 230.7 MiB**
(`process_peak_rss`) against a 128 MB budget, and a single 151.5 MiB sample at
`2026-08-03T04:52:28Z`. That is a bounded transient, not a leak — the residual
is flat around it — and it belongs to its own task. Prior art for the same
phenomenon is already recorded in-code at
[`crates/fah-rules/src/lifecycle/mod.rs`](../../crates/fah-rules/src/lifecycle/mod.rs)
`fetch_and_commit`: **~158 MiB measured at 0.2.8**.
