# A stale entry answers immediately, and refreshes in the background

Until now a stale cache entry cost the client a **full upstream round trip**.
The resolve path matched only a *fresh* hit; a stale entry fell through to the
forwarder, and the cached copy was consulted a second time only if that forward
failed. The answer we already had sat there while the client waited 20–50 ms
for the network.

Now a stale hit is answered from cache at cache-hit latency, and a refresh job
goes to a fixed-size pool of detached workers that the query path never awaits.
`[dns.cache] swr_workers` sizes the pool; `0` restores the old behaviour.

This **reverses** the rule CONTEXT.md carried since Phase 1 — that a stale entry
"answers only after a failed forward" — which is why this is an ADR and not a
doc edit.

## What this buys

Deterministic latency for stale hits, which is golden rule 8 ("avoid work with
unbounded tails on the query path") applied to the one cache state that still
had one. A popular name expiring is the common case, not an edge: every client
that asks in the window between expiry and the next successful refresh used to
pay full price, and they all paid it in parallel.

It also removes a thundering herd. N simultaneous queries for one expiring name
previously produced N forwards; they now produce one refresh, because the claim
that dedupes them is taken under the cache shard lock the lookup already holds.

## Considered options

- **RFC 8767 §5's client-response timer** — forward first, race it against a
  ~1.8 s deadline, serve stale only if the upstream is slow. This is what the
  RFC actually prescribes, and it never serves stale data unnecessarily. Rejected
  because it still pays an upstream RTT on every stale hit when things are
  healthy, which is precisely the cost this change exists to remove. What we
  implement is closer to HTTP's `stale-while-revalidate` (RFC 5861) than to
  RFC 8767, and the docs should say so rather than claim the RFC's name.
- **A global in-flight set** (`Mutex<HashSet<CacheKey>>`) for deduplication —
  rejected under golden rule 5 ("no global locks"). The cache is sharded
  specifically to avoid one, and a set consulted on every stale hit would
  reintroduce it. The claim lives in the `Entry` instead, so it costs no extra
  lock and no extra allocation.
- **Prefetching before expiry** — refreshing an entry while it is still fresh
  would avoid serving stale data at all. Rejected for now as a larger behaviour
  change that also refreshes entries nobody asks for again; the stale window is
  where the measurable cost is.
- **A dedicated Tokio runtime for the workers** — deferred. See "on priority".

## The cost, stated plainly

**A client can receive a stale answer while the upstream is perfectly healthy.**
That is the trade, and it is bounded: the reply carries `STALE_SERVE_TTL` (30 s),
so the asking resolver comes back soon, and by then the background refresh has
almost certainly landed. `MAX_STALE` (24 h) still caps how old a served answer
can be.

`Entry` grows by one `Option<Instant>` (~16 B). The cache's byte accounting
counts `size_of::<(CacheKey, Entry)>()` per *bucket*, so the real cost is 16 B ×
buckets — about 262 KB at the default 10 000 entries, ~2.6 MB at 100 k. Measured
against `max_bytes`, not assumed.

The `cache_hit` ratio in `/api/v1/telemetry` and `/api/v1/stats` will visibly rise, because
stale serves count as hits (they always did — there are simply more of them now).
That is not an anomaly and not an improvement in cache efficiency; it is this
change moving queries out of the forwarded bucket.

## On "lower priority than serving"

Tokio has no priority scheduler, and neither a `yield_now()` nor a spawn order
gives one. The guarantee is structural, and is exactly three properties:

1. **The pool is fixed at `swr_workers`.** At most that many refreshes are in
   flight, whatever the query rate.
2. **Refreshes are I/O-bound.** A worker serializes a query and parks on the
   socket. The contended resources are upstream bandwidth and the shard locks,
   not CPU.
3. **The query path never awaits the pool.** Enqueue is `try_send`; a full queue
   drops the job and keeps serving stale. It can never become backpressure.

If measurement ever shows refreshes displacing serving, the escalation is a
dedicated runtime with its own thread. Not built, because nothing yet says it is
needed.

## Failure behaviour

A refresh that fails — transport error, or an answer the cache declines such as
SERVFAIL or a truncated reply — puts the entry into a 30 s cooldown
(`REFRESH_FAILURE_COOLDOWN`, matching `STALE_SERVE_TTL`). Without it a dead
upstream would turn every stale hit into a forward, which is worse than the
behaviour this change replaced.

The claim is a **lease** (`REFRESH_CLAIM_LEASE`, 5 s), not a flag. A worker that
panics or is aborted at shutdown therefore cannot strand an entry: the claim
expires and the key becomes refreshable again. The lease must outlast a
worst-case forward — `[dns.upstreams] timeout_ms` × the number of servers, which
is 1.6 s at the shipped defaults. A config with six or more upstreams at the
default timeout narrows that margin, and the failure mode there is one duplicate
forward, never a wrong answer.

## Revisit criteria

- Revert to `swr_workers = 0` if stale answers cause a visible correctness
  problem — a service whose records move faster than 30 s and that clients reach
  through us.
- Revisit the pool size if `fastadhunter_swr_refreshes_dropped_total` grows
  steadily on-device: the queue is being outrun and either the workers or the
  queue depth are undersized.
- Revisit the whole approach if `_failed_total` tracks `_enqueued_total` — that
  would mean refreshes are systematically failing where client-driven forwards
  succeed, and the synthetic refresh query would be the first suspect (it carries
  no EDNS options from the client that triggered it).
- Reconsider RFC 8767 §5's timer if serving stale under a *healthy* upstream
  ever proves to matter more than the latency it costs.
