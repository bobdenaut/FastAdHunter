# Scheduled cache cleanup: a sweep, and the ghost nodes it found

**Date:** 2026-07-31 · **Config:** `[dns.cache] cleanup_interval_seconds`
(default 360, `0` disables) · **No new files**

---

## 1. Most of this already existed

The request was "a background task that evicts expired DNS cache entries,
incremental, no global lock, must not remove serve-stale entries". Nearly all of
that was already true of `DnsCache::clean(purge_stale)`
(`crates/fah-dns/src/cache.rs`), which has been serving
`POST /api/v1/cache/clean` since p1-09: it locks one shard at a time, keeps
`EntryState::Stale` when `purge_stale = false`, maintains `guard.bytes`, and
returns a `CacheClean` carrying counts, freed bytes and duration.

So the emphasised requirement — *"only entries beyond the stale window should be
deleted"* — needed no code. It is what `false` already means, and the background
task calls the same function rather than a second walk. No
`clean_expired_only`, no duplicated retain predicate.

What was genuinely missing: a scheduler, a config key, metrics — and one memory
bug that had been sitting in the admin clean path all along (§3).

## 2. The honest part: at defaults this sweep will find nothing

With `serve_stale = true` (shipped default), `Entry::state` returns `Expired`
only past `expires_at + MAX_STALE`, and `MAX_STALE` is **24 hours**. A sweep
every 360 s therefore only ever reaches entries whose TTL lapsed more than a day
ago — and under any real query rate, FIFO capacity eviction has taken those long
before.

This was raised before implementing and the decision was to ship it anyway, with
no load thresholds and no heuristics. That is the right call for two reasons
worth writing down:

- The case it *does* serve is real and nothing else covers it: a cache that goes
  idle **below both caps** — a household resolver overnight — never evicts, so
  its dead entries are resident until restart.
- The scheduler is the asset. If `MAX_STALE` becomes configurable or another
  retention policy lands, the task exists and needs no redesign.

The cost of being wrong is a walk of a mostly-fresh cache every six minutes,
which is microseconds. Recorded at `default_cleanup_interval_seconds` and in
CONFIGURATION.md §"The cleanup sweep is not a third bound" so nobody later reads
`cleanup_entries_removed_total ≈ 0` as a fault.

**It is not a bound.** `max_entries`/`max_bytes` bound the cache and hold with
the sweep disabled. This returns memory *underneath* them.

## 3. The find: `clean` freed entries but stranded their queue nodes

The eviction queue holds one `(CacheKey, seq)` node per insert, each owning a
**cloned domain string**, and `queue_bytes` counts them — so they are real,
reported memory. `clean`'s `map.retain` removed entries without touching the
queue, leaving one ghost node each.

`Shard::compact` sweeps ghosts, but only above `queue.len() > capacity * 2`, and
it is only ever called **from `insert`**. Put those together:

> A cache that goes quiet after a sweep never inserts again, so its ghosts are
> never collected.

That is precisely the cache this whole feature targets. The bug and the feature
had the same trigger condition.

Fixed by factoring the "is this node live" predicate into `Shard::sweep_queue`,
which `compact` now delegates to behind its existing gate, and which `clean`
calls ungated on any shard that removed something. `VecDeque::retain` compacts
in place — no reallocation, no copy of survivors — so the cost is one hash
lookup per node, the same order as the `map.retain` that just ran. Gated on
`removed > 0` so a sweep that finds nothing stays a single walk.

This also fixes the admin `POST /api/v1/cache/clean`, which had the same gap.

## 4. `shrink_to_fit` is deliberately absent

Returning the **slabs** — `map.shrink_to_fit()` (realloc + full rehash + copy)
and `queue.shrink_to_fit()` — was in the original plan and was cut on review:
at 100 k entries a rehash every six minutes could plausibly cost more than the
memory it returns, and that has never been measured on a 1.4 GHz ARM core.

Consequences, stated rather than papered over:

| After a sweep | Returned? |
| ------------- | --------- |
| Removed entries' own heap (`freed_bytes`) | yes |
| Their queue nodes' cloned domains | yes — new in this change |
| Hash-table slab (`table_bytes`, counted per *bucket*) | **no** |
| Queue ring capacity | **no** |

So `cache_estimated_bytes` now falls where it previously stayed flat, but not by
the slab. A cache that filled to 100 k and emptied keeps its 100 k-bucket table
— bounded by `max_entries`, just not returned.

**The decision instrument ships with the change.**
`fastadhunter_cache_cleanup_duration_seconds` at real on-device occupancy is the
"before" number the A/B needs, for free. Take it after 0.2.9 is deployed; if a
shrink proves worth it, the gate is one line
(`map.len() * 2 < map.capacity()`) at the same call site. Written into
`clean`'s doc comment so a future reader does not add one on intuition.

Either way, freed memory goes back to **mimalloc**, not necessarily to the
kernel — RSS lags `cache_estimated_bytes` (CONTEXT.md §Accounted/Residual).

## 5. Counters live inside `clean`, so the admin path counts too

Four `AtomicU64` on `DnsCache`, bumped in `clean` itself. That means
`POST /api/v1/cache/clean` ticks them as well. Deliberate: one code path means
the counters cannot report something that did not happen to the cache. Said so
in the metric HELP text rather than hiding it.

| Series | Type | |
| ------ | ---- | --- |
| `fastadhunter_cache_cleanup_runs_total` | counter | sweeps completed, scheduled **and** admin |
| `fastadhunter_cache_cleanup_entries_removed_total` | counter | entries removed |
| `fastadhunter_cache_cleanup_bytes_freed_total` | counter | entry heap returned |
| `fastadhunter_cache_cleanup_duration_seconds` | gauge | wall time of the **last** sweep |

`bytes_freed_total` is the one to graph, and it was added on review for a good
reason: entry sizes differ by an order of magnitude between an A record and a
large TXT/SOA, so a removal count says little about what was reclaimed — and
`max_bytes` means bytes are the units the cache is actually governed in.

A gauge rather than a histogram for duration: at a 360 s cadence there is at
most one sweep per scrape, so the last value hides no outlier, and 240
observations a day do not justify twelve buckets.

`CleanupSnapshot` mirrors `CacheCleanupStats` across the `fah-dns` ↔
`fah-metrics` sibling boundary, copied field-by-field by the binary in
`spawn_telemetry_poll` — the same shape `SwrSnapshot` already uses, for the same
layering reason. Stored as one `ArcSwap` so a scrape cannot land mid-update and
show bytes freed by a run that has not been counted.

## 6. Every sweep logs the whole `CacheClean`

```text
cache cleanup complete expired_removed=… stale_removed=… bytes_freed=…
                       entries_before=… entries_after=… duration_us=…
```

Logging only what was removed cannot distinguish "the sweep ran and found
nothing" from "the sweep never ran" — which at default settings is exactly the
question being asked. `stale_removed` is included despite being structurally
always `0` for a scheduled sweep: that zero is the on-device proof that
serve-stale entries are being left alone.

Two arms, `info` when something was removed and `debug` when not, because
`tracing` fixes the level at compile time. 240 `info` lines a day saying nothing
would cost real history on the RouterOS log buffer.

## 7. Placement: the blocking pool, not a DNS worker

`clean` is synchronous and O(entries). At a raised `max_entries` a full walk is
exactly the unbounded tail PERFORMANCE.md golden rule 8 exists to keep off the
query path, so the sweep goes through `spawn_blocking` — the same treatment, for
the same reason, the p2-07 memory pass got.

Shard lock hold time is unaffected either way: one shard at a time, so a
concurrent resolve waits at most one shard's walk. `MissedTickBehavior::Skip`,
and the ticker's immediate first tick is consumed so the sweep does not run
against a cache that has been up for milliseconds.

A panicking sweep is logged and the scheduler continues — surviving one bad
sweep is worth more than the entries it would have removed.

## 8. Three traps hit while building this

**Paused-clock tokio does not advance while a spin loop runs.** The first
scheduler tests waited on `yield_now()` and timed out: tokio's auto-advance
requires the runtime to be *idle*, and a spin loop never is. The test helper now
drives `tokio::time::advance` explicitly, with `Duration::ZERO` meaning "just
yield" for waits whose progress comes from another task rather than from time.

**`swr_workers = 0` changes what a stale hit does.** A cleanup test set it to 0
to keep the refresh pool from touching the entries under test, then asserted a
stale hit was served from cache — which fails, because with the pool off a stale
entry only answers *after a forward fails* (the pre-ADR-0005 path). Rewritten to
kill the upstream first, which is both correct and a better test: it proves the
surviving entry is genuinely usable during the outage it is insurance for, not
merely resident.

**One existing test pinned the old byte arithmetic.**
`byte_estimate_counts_the_table_slab_not_just_occupied_entries` asserted
`estimated_bytes == one - freed_bytes` after a clean. That is now 16 B lower,
because the ghost domain comes back and `freed_bytes` is an entry-heap figure
that never counted it. The assertion now derives the ghost-domain term from the
key rather than hardcoding a number, so it stays honest if domain lengths change.

## 9. Tests — 11 new, 1 amended

| Where | What it pins |
| ----- | ------------ |
| cache | no ghost queue nodes remain after a sweep |
| cache | `estimated_bytes` falls, by at least the reported entry heap |
| cache | counters track every sweep, including one that removed nothing |
| cache | an admin stale purge counts too |
| cache | sweeping under 4 concurrent readers: no deadlock, fresh entries keep answering, `bytes` matches a fresh walk |
| pipeline | the scheduler removes dead entries with nothing prompting it |
| pipeline | **a serve-stale entry survives repeated sweeps and is still usable** |
| pipeline | a sweep and a pending SWR refresh do not fight |
| pipeline | `cleanup_interval_seconds = 0` spawns no task |
| config | default 360; `0` disables without touching the caps |
| metrics | all four series exported from the first scrape, µs→s converted |

The `estimated_bytes` test asserts a *decrease*, not an exact figure —
deliberately, since pinning a number would encode today's non-shrinking slab as
a promise (§4).

## 10. Bench: nothing moved, as expected

The resolve path is untouched — `lookup` is unchanged, `Entry` is unchanged, and
`compact` gains one function-call indirection. Two pinned runs of the cache-hit
bench (one core, high priority, per PERFORMANCE.md §Measuring reliably):

| Run | median | change | |
| --- | ------ | ------ | --- |
| 1 (n=100) | 3.053 µs | +5.0% `[-18.2%, +36.8%]` | p = 0.70 |
| 2 (n=200) | 2.870 µs | −4.7% `[-27.6%, +23.2%]` | p = 0.70 |

"No change in performance detected" both times, and the sign flips between runs.
The intervals are wide enough to be noise wearing a number's clothes — but the
mechanism agrees: there is no hot-path change to regress.

## 11. On-device — 0.2.9, 2026-07-31 (partially verified; soak running)

Deployed 08:30, `cache cleanup scheduler started` in the boot log. After ~40
minutes:

| | |
| --- | ---: |
| `cleanup_runs_total` | 3 |
| `cleanup_entries_removed_total` | 0 |
| `cleanup_bytes_freed_total` | 0 |
| `cleanup_duration_seconds` | **0.000079** |

`runs` climbing at one per 360 s with `entries_removed` flat is exactly the
reading §2 predicted, and it is what the run counter exists to express: *the
sweep is alive and there is nothing dead yet*. Nothing in a 40-minute-old cache
can be 24 h past its TTL.

**The sweep costs 79 µs** for a full 16-shard walk at 409 resident entries —
about 19 ms of CPU per day at the default cadence. Free, at this occupancy.

**What this does NOT yet settle.** The walk is O(entries) and 409 is not a
scale sample, so 79 µs cannot be extrapolated to the `shrink_to_fit` question
in §4. A 24 h soak is running (31/07 08:30 → 01/08 08:30) to get the figure at
a full day's occupancy, and that is the number the decision needs.

**Still unobserved, by construction:** `bytes_freed_total` going non-zero. The
earliest that can happen is ~01/08 07:55 — 24 h after the first entries cached
at boot expire. Until then the feature has been proven to *run*, not to *do
anything*, and the review should not claim otherwise.

One measurement trap recorded because it cost a wrong call already:
`/system/resource/print` reports the **router's** uptime, not the container's.
Reading `1d19h` there as container age led to looking for reclaimed bytes about
23 hours too early.
