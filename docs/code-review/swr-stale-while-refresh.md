# Stale-while-refresh: a detached refresh worker pool

**Decision record:** [ADR-0005](../decisions/0005-serve-stale-while-refresh.md) ·
**Date:** 2026-07-31 · **New file:** `crates/fah-dns/src/swr.rs`

---

## 1. What was wrong

`Pipeline::resolve` matched only `Lookup::Fresh` on its first lookup. A **stale**
entry fell straight through to `forwarder.forward(request).await`, and the cached
copy was consulted a second time *only if that forward failed or returned
SERVFAIL*. The answer was sitting in memory while the client waited 20–50 ms for
the network.

Worse at scale: N simultaneous queries for one expiring popular name produced N
forwards. Nothing coordinated them.

## 2. What it does now

A stale hit is answered from cache at cache-hit latency with a short
`STALE_SERVE_TTL` (30 s), and a refresh job goes to a fixed pool of detached
workers sized by `[dns.cache] swr_workers` (default 3). The query path offers the
job with `try_send` and never awaits anything.

`swr_workers = 0` restores the previous behaviour exactly — including the
failed-forward stale fallback — and is tested as such.

**Naming, deliberately:** this is closer to HTTP's `stale-while-revalidate`
(RFC 5861) than to RFC 8767, whose §5 prescribes forwarding first and racing a
~1.8 s timer. That option was considered and rejected in ADR-0005 because it
still pays an upstream RTT on healthy stale hits, which is the whole cost being
removed. Docs no longer call this "RFC 8767 serve-stale".

## 3. Deduplication is the design, and it does not use a global lock

The obvious dedup — `Mutex<HashSet<CacheKey>>` — is what golden rule 5 forbids,
and the cache is sharded specifically to avoid one.

Instead the claim lives in the entry:

```rust
struct Entry {
    ...
    refresh_suppressed_until: Option<Instant>,
}
```

`lookup_and_claim_refresh` reads the entry and takes the claim **inside the shard
lock the read already holds** — no extra lock, no extra allocation, no global
state. Exactly one caller per lease gets `claimed_refresh: true`.

One field serves two purposes, which is what makes it robust:

| Purpose | Set to | Why |
| ------- | ------ | --- |
| **Claim** | `now + REFRESH_CLAIM_LEASE` (5 s) | dedup while a worker is refreshing |
| **Cooldown** | `now + REFRESH_FAILURE_COOLDOWN` (30 s) | a dead upstream cannot turn every stale hit into a forward |

Making the claim a **deadline rather than a flag** is the part that matters: a
worker that panics or is aborted at shutdown cannot strand its entry, because a
claim that outlives its lease reads as unclaimed. There is no "unclaim on
success" step to forget either — a successful refresh replaces the whole `Entry`,
and fresh entries carry `None`.

## 4. No duplicates, checked four ways

The explicit review concern. Each is closed by construction, not by care:

1. **Duplicate refresh jobs** — the claim above. Tested at the cache level (64
   repeated claims yield one) and end-to-end through the pipeline (50 concurrent
   stale hits → `enqueued == 1`, `deduplicated == 49`, `forwards == 0`).
2. **Duplicate cache entries** — the worker calls the *existing* `store` →
   `insert`. That replaces the map entry, subtracts the previous entry's bytes
   from the shard total, and takes a new `seq` so the old queue node becomes a
   ghost the existing `compact()` sweeps. No second store path was written.
3. **Duplicate code** — `lookup` and `lookup_and_claim_refresh` both delegate to
   one `lookup_inner`, so the freshness rules cannot drift between a claiming and
   a non-claiming read. `store` gained a `bool` return rather than growing a
   near-copy `store_refreshed`. The refresh reuses the same `Forwarder` the
   pipeline uses.
4. **Duplicate forwards from a lapsed lease** — possible by design, and bounded.
   The lease must outlast a worst-case forward (`timeout_ms` × upstream count =
   1.6 s at shipped defaults, so 5 s is ~3× headroom). A config with six or more
   upstreams at the default timeout narrows that, and the cost is **one duplicate
   forward**, never a wrong answer or a stuck entry. Recorded at the constant so
   nobody later "fixes" it by deriving it from `[dns.upstreams]` and coupling the
   cache to upstream config.

## 5. Two traps hit while building this

Both would have shipped silently.

**A refresh can succeed and still leave the entry stale.** `store` declines
SERVFAIL, REFUSED and truncated replies. Counting a successful *forward* as a
successful *refresh* would clear the claim while the entry is still stale, and
the next hit would re-enqueue the same doomed refresh — forever, against a broken
upstream. The worker's success condition is therefore "the upstream answered
**and** the cache kept it"; anything else takes the cooldown.

**The first test harness invalidated its own assertions.** It made the seed
entry stale by setting `max_ttl_seconds: 0` — which also clamped the *refreshed*
answer to zero TTL, so "the entry is fresh again" could never hold. Caught
because that assertion failed; had it been written more loosely it would have
passed while testing nothing. Now the harness uses a paused clock and ages the
entry with `tokio::time::advance`, with the doc comment saying why.

## 6. On "lower priority than serving"

Stated honestly in the module docs and in ADR-0005: **Tokio has no priority
scheduler.** Neither `yield_now()` nor spawn order provides one. The guarantee is
structural and is exactly three properties:

1. The pool is **fixed at `swr_workers`** — at most that many refreshes in
   flight, whatever the query rate.
2. Refreshes are **I/O-bound** — a worker serializes a query and parks on the
   socket. Contended resources are upstream bandwidth and shard locks, not CPU.
3. The query path **never awaits the pool** — `try_send` only; a full queue drops
   the job and keeps serving stale, so it can never become backpressure.

`yield_now()` before each forward is kept as a cooperative gesture and labelled
as such, not as a mechanism.

## 7. Bench: no measurable regression

The cache-hit bench is the direct site of the change (`lookup` gains a branch,
`Entry` grows 16 B). Interleaved A/B against a clean `HEAD` worktree build,
pinned to one core at high priority, alternating to cancel drift:

| Round | baseline | current |
| ----- | -------- | ------- |
| 1 | 2.957 µs | 3.037 µs |
| 2 | 2.858 µs | 3.163 µs |
| 3 | 3.104 µs | **2.901 µs** |

Median-of-medians **+2.7%**, confidence intervals fully overlapping, and round 3
has the new code faster than the baseline. The order flip is the tell: this is
noise, not a regression. Well inside the 10% gate.

**Methodology note worth keeping.** The first run reported `+141% Performance has
regressed` — measured while a video was playing on the box. Three consecutive
runs of *identical* code then swung −42% to +43%. PERFORMANCE.md §Measuring
reliably already warns about this; it cost a detour anyway. Interleaving against
a real baseline build is what turned an unusable number into an answer.

## 8. Cost

- **Memory:** one `Option<Instant>` (~16 B) per `Entry`. The byte accounting
  charges `size_of::<(CacheKey, Entry)>()` per *bucket*, so ~262 KB at the
  default 10 000 entries and ~2.6 MB at 100 k. Recorded in PERFORMANCE.md against
  `max_bytes` rather than assumed free.
- **Queue:** `swr_workers × 64` slots, bounded by configuration and never by
  traffic or uptime (hard rule 4).
- **Observability shift:** the `cache_hit` ratio will visibly **rise** and the
  forwarded-query rate fall. That is queries moving between buckets, not the
  cache becoming more efficient. Called out in PERFORMANCE.md so nobody reads it
  as an improvement or an anomaly.

## 9. Tests — 13 new

| Where | What it pins |
| ----- | ------------ |
| cache | one of 64 repeated claims wins |
| cache | a plain `lookup` never takes the claim |
| cache | a claim outliving its lease is re-claimable (the dead-worker case) |
| cache | a failed refresh suppresses the next for the cooldown, then stops |
| cache | a released claim is immediately available |
| cache | a successful refresh clears the claim by replacing the entry |
| cache | `store` reports cacheability (positive / SERVFAIL / truncated) |
| swr | a refresh replaces the entry and counts once |
| swr | SERVFAIL and transport failure both take the cooldown |
| swr | a full queue drops the job **and releases the claim** |
| swr | workers drain the queue; a second `spawn` is a no-op |
| swr | the refresh query reproduces name, type **and class** |
| pipeline | a stale hit answers without forwarding, with the stale TTL |
| pipeline | 50 stale hits → 1 job, 49 dedup, 0 forwards |
| pipeline | `swr_workers = 0` forwards as before |
| pipeline | `swr_workers = 0` still serves stale on a failed forward |
| pipeline | a queued refresh runs and the entry is fresh again |
| config | default is 3; `0` disables without disabling `serve_stale` |
| metrics | all five series exported from the first scrape, tracking the snapshot |

Lease and cooldown tests assert against the constants, not literals, so retuning
them cannot silently invalidate the tests.

## 10. Known edge, accepted

A job whose entry is evicted before the worker runs will **resurrect** that key:
`store` inserts a fresh entry for a name nothing currently wants. Bounded by the
cache's own limits and harmless, so it is left alone rather than guarded with a
second lookup on the worker side.

## 11. Not verified on-device

Local only. On deploy, read from `/metrics`:

- `fastadhunter_swr_refreshes_completed_total` climbing — the pool is working.
- `_dropped_total` at or near zero. Sustained growth means `swr_workers` is
  undersized, **not** that anything is failing.
- `_failed_total` tracking `_enqueued_total` would be the real alarm: it would
  mean refreshes fail where client-driven forwards succeed, and the synthetic
  refresh query is the first suspect (it carries no EDNS from the client that
  triggered it).
- `GET /api/v1/cache` — the `stale` count should stop growing without bound.
