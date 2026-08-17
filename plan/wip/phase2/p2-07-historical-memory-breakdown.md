# P2-07 — Historical Memory Breakdown

**Phase:** 2 · **Depends on:** — · **Model:** Opus

> Not HTTP work, like `p2-00`: the `p2-` prefix is a scheduling position. Placed
> immediately before `p2-08` because Phase 2's soak is the first consumer — a
> soak that can only report RSS answers "is it growing?" but never "growing
> *where*?".

## Status — code complete 2026-08-02, `AWAITING SOAK`

**Everything this file asks for has shipped except its two on-device criteria.**
Commit `0cfa317`, report `docs/code-review/phase2/p2-07-review.md` §11.

| Asked for | State |
| --------- | ----- |
| `Matcher::heap_bytes()`, cache `bytes` | shipped (first pass) |
| `StatsHeap` — aggregates, clients, ring, pending_log | shipped, all four fields |
| `fastadhunter_memory_component_bytes`, `_residual_bytes` | shipped |
| `/api/v1/debug/memory` | shipped |
| Single-instant sampling on the telemetry poll | shipped |
| Allocator figures (`AllocatorStats`) | shipped 0.2.8, live only |
| `MemoryComponents` in `PerfSample` + `/history/perf` | **shipped 2026-08-02** |
| Residual against a fresh mimalloc baseline | **needs the RB5009 — p2-08** |
| Residual slope over a soak window | **needs the RB5009 — p2-08** |

**What flips this to `DONE`:** the p2-08 soak, pulling the series from
`/history/perf` **on-device** (not an external curl loop — the point is that the
router self-hosts it), confirming a residual per sample across the window and a
row agreeing with a `/debug/memory` read at the same instant, then **stating the
slope over the final third**. A *drifting* residual still flips this to `DONE` —
the instrument worked — and opens a leak task with the series attached. Fixing a
leak was never in this task's scope.

### The reopen's motivating question has been answered — the instrument is still worth building

This file was reopened on 2026-07-26 because the 0.2.5 soak could not say
whether the router's memory climb was a leak. **That specific question is now
closed:** the climb was cgroup v2 charging page cache, reconciled against
on-disk data to within 0.06 %, and confirmed independently by a container
restart dropping `memory-current` from 595.0 to 60.1 MiB. See
`docs/code-review/phase2/0.2.7-router-memory-and-throughput.md` §5–§6.

So the urgency is gone, and the honest framing changes with it: this is no
longer an investigation, it is a **safety net**. What it catches is a leak of a
few bytes per query — invisible to any sampling rate a human sustains by hand,
and still entirely possible. Build it before the Phase 2 soak, not because
something is suspected, but because the soak is worthless as evidence without
a series to read.

Priority accordingly: lower than when it was reopened, unchanged in position.

### What moved under this file since it was written

- **The 0.2.4 baseline in *Context* below is void.** Those numbers were measured
  under musl `mallocng`. mimalloc replaced it in 0.2.8, and pre-swap RSS and
  residual series are not comparable to post-swap ones. Re-measure before
  quoting a percentage.
- **The field set changed.** `allocator_retained_bytes` was **removed** in
  0.2.8 — it reported 260 MiB of "retention" in a process with 70 MiB resident,
  because mimalloc v3 never decrements its commit counter when a purge returns
  pages, so `committed − accounted` only ever rises. `minor_page_faults` was
  added in its place. CONTEXT.md records "allocator retained" as a retired
  term; do not reintroduce it.
- ~~**`fastadhunter_memory_collection_seconds` goes sooner than this file
  says.**~~ **REMOVED 2026-08-02** in `0cfa317` — gauge, setter, atomic and the
  `TEMPORARY` block together. Last on-device reading `0.000986` (986 µs), and
  the pass runs on `spawn_blocking`, off the DNS workers, so its duration no
  longer reaches query latency.

## Goal

Every bounded structure reports its own heap, so `RSS − Σ(components)` is a
small, stable **residual**. A leak then shows as a growing residual while every
named component stays flat — a far sharper signal than watching RSS. The
components report this already; only the *series* is missing.

## Context

**Superseded — retained to show what the residual looked like before the
allocator swap. Do not use as a baseline.** Measured on the RB5009 after 10 h of
household traffic on 0.2.4, under musl `mallocng`:

```text
RSS               43,700,224 B   41.68 MiB
  ruleset heap   −23,002,595 B   21.94 MiB
  cache bytes    − 1,053,072 B    1.00 MiB
                 ─────────────
  unexplained     19,644,557 B   18.73 MiB   (45 % of RSS)
```

The most recent on-device figures, 0.2.8 under mimalloc, for orientation only —
the component split at that instant was not captured, which is precisely the
gap this task closes:

```text
RSS                          73,707,520 B    70.29 MiB
allocator current_commit    318,046,208 B   303.31 MiB   (accumulating, not live)
allocator peak_rss          157,990,912 B   150.67 MiB   (getrusage high-water)
minor page faults             4,211,337                  (rate is the purge-thrash signal)
```

`current_commit` exceeding `peak_rss` by 150 MiB is the normal state, not an
accounting fault — `crates/fah-model/src/memory.rs` has a test asserting exactly
that, so nobody re-derives it as a bug.

Hard rule 4 is "bounded everything". If everything is bounded, everything should
be able to say how big it is — and the ones that grow with *uniqueness* rather
than traffic are already capped and tested
([`ClientRegistry`](../../../crates/fah-stats/src/client_registry.rs) at 4,096
with LRU eviction; `top_n` at `HOURLY_TRACKED_DOMAINS = 256`).

## Scope

### Already delivered — do not redo

The per-component `heap_bytes()` work, the two metric families, `/debug/memory`,
and single-instant sampling all shipped in the first pass. Two decisions from
that pass are load-bearing and must survive this change:

- **Track the large single-mutation-point structures; walk the small intricate
  ones.** `Stats::heap()` cost 80 µs walking everything and 43 µs once the query
  ring kept a running total: the ring is 16,384 entries with exactly one mutation
  point, so tracking is both the big win and the safe one. `top_n` (≤256
  keys/slot) and `ClientRegistry` (≤4,096) keep being walked — their eviction is
  multi-step, which is where a running total drifts. Any running total needs a
  test asserting it equals a full walk after eviction. (See
  `docs/code-review/phase2/p2-07-review.md` §5.)
- **Each `heap_bytes()` documents what it excludes.** `p1-02-review.md` §4 found
  `heap_bytes` counting `Arc` control blocks but not their payload strings — an
  undercount in the very number feeding a budget claim. An unstated exclusion
  silently becomes residual.

### The remaining work — persist the breakdown

- **Split the components out of `MemoryBreakdown`** so one field list serves
  both the live path and the persisted row:

  ```rust
  pub struct MemoryComponents { ruleset, cache, stats: StatsHeap }  // + serde
  pub struct MemoryBreakdown  { components: MemoryComponents, rss: Option<u64>,
                                allocator: Option<AllocatorStats> }
  ```

  `accounted()` moves to `MemoryComponents`; `residual()` stays on
  `MemoryBreakdown` and delegates.

- **`PerfSample` gains `memory: MemoryComponents`** — components only. It keeps
  its existing `rss_bytes`, and the two rules below are what keep the row free
  of anything meaning the same thing twice:
  - **No `rss` in the persisted breakdown.** `PerfSample.rss_bytes` is already
    the RSS, is already in every existing history file, and is already a
    documented `?fields=` selector. Removing or duplicating it would either
    orphan the RSS in 30 days of existing rows (serde drops unknown fields) or
    store the same number twice.
  - **Do not persist `residual`.** It is `rss_bytes − memory.accounted()`,
    computed on read by the same function the live path uses. A stored residual
    is a derived value that can silently disagree with its own inputs after any
    change to what a component counts.

- **Persist `minor_page_faults`, and nothing else from `AllocatorStats`.** The
  test is whether a *series* of the figure says more than its latest value does:
  - `minor_page_faults` — **persist.** It is a monotone counter whose
    **derivative** is the signal: a rising fault rate at flat RSS is mimalloc
    purging and re-faulting the same pages, which is the one allocator pathology
    a soak can actually catch. A rate needs consecutive rows; a single `curl`
    cannot produce one.
  - `current_commit`, `peak_commit`, `peak_rss` — **do not persist.** All three
    are monotone non-decreasing over process lifetime, so as a series they are
    ramps that carry no information the newest reading does not already give.
    `peak_rss` is a budget check against ≤ 128 MB, which `/metrics` serves live.
  - Put it on `PerfSample` directly rather than inside `MemoryComponents`: it is
    a kernel process counter, not a component's heap, and `accounted()` must not
    be able to pick it up.

- **`#[serde(default)]` on every new field.** Existing
  `/data/history/perf/perf-YYYY-MM-DD.jsonl` rows have no `memory` and no
  `minor_page_faults` key and must keep parsing; `MemoryComponents` is `Default`.

- **The sampler already has the value.** `main.rs` builds the breakdown once per
  telemetry poll inside a `spawn_blocking` pass; hand that same instance to the
  perf sample rather than collecting twice — the single-instant rule applies
  here too, and the pass is deliberately off the DNS workers.

- **`/api/v1/history/perf`**: add `memory` and `minor_page_faults` to
  `PerfSampleResponse` and to `PerfFields` (including the `?fields=` name list
  and `ALL`), and serve the computed residual per row. API.md updated in the
  same change.

- Disk cost: ~7 MB per 30 days at `sample_interval_seconds = 60`, pruned by
  `history.retention_days` like every other row. Note it in CONFIGURATION.md
  beside the existing history sizing guidance.

## Acceptance criteria

- **The residual is never negative.** A negative residual means a component
  double-counts or over-reports; assert it in a test with a populated cache,
  ruleset and stats, and log at `warn` if it ever happens in production.
- Residual is a *small* fraction of RSS and **stable over a soak window**. The
  proof is a **series**: after a multi-hour run, `GET /api/v1/history/perf`
  returns one residual per sample interval across the whole window, and the
  verdict is stated as a slope over its final third, not as a difference between
  two readings. *(The original criterion asked for two readings, which is what
  made the 0.2.5 soak inconclusive.)*
- **State where the residual lands against a freshly measured mimalloc
  baseline.** The 45 % figure above predates the allocator swap and must not be
  quoted as the number being improved on. Say what remains unaccounted — binary
  pages, thread stacks and allocator slack legitimately do.
- A history file written *before* this change still parses, and its rows return
  `rss_bytes` with an absent/zero `memory` — old data must not become
  unreadable.
- Live and persisted residual agree: a row sampled at time *t* and a
  `/debug/memory` read at the same instant yield the same figure, because both
  go through `residual()`.
- Component sum tracks reality: filling the cache to its `max_bytes` moves
  `component_bytes{component="cache"}` by the expected amount and leaves the
  residual flat.
- **No hot-path cost.** Accounting is read on the telemetry poll only; a bench
  or allocation-counter test shows the query path is unchanged.
- Gates green.

## Out of scope

Fixing any leak this finds — that is a follow-up with evidence attached.
Disk accounting for the query log and history (`retention_max_mb` already
bounds that, and CONFIGURATION.md documents the overshoot). Swapping the
global allocator for `dhat`-style profiling: it is a build-level change,
unavailable on the musl static target in production, and gives a one-off
capture where this gives a permanent signal. Re-opening the RouterOS
`memory-current` question — resolved, see the Status section.

## Suggested prompt

> Read plan/wip/phase2/p2-07-historical-memory-breakdown.md — note that the
> instrumentation half already shipped and only persistence is left. Split
> `MemoryComponents` out of `MemoryBreakdown`, add it plus `minor_page_faults`
> to `PerfSample` behind `#[serde(default)]`, hand the sampler's existing
> single-instant breakdown to the perf writer rather than collecting twice, and
> expose both through `/api/v1/history/perf` with the residual computed per row.
> Prove old history files still parse and that live and persisted residual agree.
