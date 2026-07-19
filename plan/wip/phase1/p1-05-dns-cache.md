# P1-05 — DNS Cache

**Phase:** 1 · **Depends on:** p1-04 · **Model:** Sonnet

## Goal

Bounded, sharded in-memory cache of upstream answers with TTL clamps,
negative caching and serve-stale.

## Context

ARCHITECTURE.md §DNS Pipeline (cache stores upstream answers only, never
verdicts) + CONFIGURATION.md `[dns.cache]` + ADR-0001.

## Scope

- Bounded size from config (`max_entries`, default 10k, runtime-mutable),
  sharded by key hash across workers' access — no global lock; eviction
  LRU-ish or S3-FIFO (implementer's choice, justify in code comment).
- Key: (qname lowercase, qtype, qclass). TTL clamps: `min_ttl`, `max_ttl`.
- Negative caching per RFC 2308 (SOA minimum, capped by
  `negative_ttl_max_seconds`).
- Serve-stale per RFC 8767 minimal form: upstream failure ⇒ expired entry
  (≤24h stale) served with short TTL, flagged in QueryEvent.
- Tests: TTL expiry (mock clock), clamp behavior, negative entries, stale
  serving on upstream error, eviction at capacity, concurrent hammering.

## Acceptance criteria

- Memory bounded: filling beyond capacity evicts, never grows (test asserts).
- Cache hit adds < 1ms in-engine latency (bench added to `benches/`).
- Gates green.

## Out of scope

Persistence (cache is RAM-only by design), prefetch/optimistic refresh.

## Suggested prompt

> Read CONFIGURATION.md §[dns.cache], ARCHITECTURE.md pipeline properties, and
> plan/wip/phase1/p1-05-dns-cache.md. Implement the sharded bounded cache with
> RFC 2308 + RFC 8767 behavior and the listed tests + bench.

## Completion note

**Design:** `crates/fah-dns/src/cache.rs` — `DnsCache`, crate-private (the
pipeline is the only caller; nothing outside `fah-dns` needs cache internals).

- **Sharding, not one lock.** 16 independently-locked (`std::sync::Mutex`)
  shards, index picked by hashing `(domain, qtype, qclass)` with a
  per-instance `RandomState` (`BuildHasher::hash_one`) — matches
  ARCHITECTURE.md §Runtime Model's "sharding for the cache" without a global
  lock. 16 is fixed (not derived from core count) — the RB5009's 4 cores and
  Phase 1's handful of listener tasks make contention negligible at that
  count already; each critical section is a few `HashMap` ops.
- **Key:** `(domain lowercased, qtype, qclass)` exactly as queried — a hit
  replays the upstream's answer records verbatim, TTL recomputed per read.
- **Eviction:** FIFO with stale-first quick-demotion, not LRU (true LRU
  requires mutating access-order on every *read*, turning a hit into a
  write-under-lock). On a full shard: an entry already past its RFC 8767
  stale deadline goes first (strictly dead weight); otherwise the true-oldest
  entry. Documented in-code as the task's "implementer's choice, justify in a
  comment" requirement.
- **TTL clamps:** positive answers use the minimum TTL across the answer
  set, clamped to `[min_ttl_seconds, max_ttl_seconds]`. Negative answers
  (RFC 2308 §5) use the SOA `MINIMUM` field from the authority section
  (falling back to the cap itself if no SOA is present), capped by
  `negative_ttl_max_seconds`. `SERVFAIL`/`REFUSED`/etc. are never cached —
  transient upstream state, not an answer worth remembering.
- **Serve-stale (RFC 8767 §4 minimal form):** an expired entry stays servable
  for up to 24h past its TTL (hardcoded — CONFIGURATION.md's `[dns.cache]`
  has no stale-window knob); only used when the forwarder's live attempt
  actually fails, and only when `serve_stale` is enabled. Served with a
  short 30s TTL. `fah_model::QueryEvent` gained a `stale: bool` field so
  consumers can tell a normal cache hit apart from "the network was down and
  this is what we had."
- **Pipeline integration** (`pipeline.rs`): the Allow/Pass path now checks
  the cache before the forwarder (`Lookup::Fresh` short-circuits it
  entirely), stores the forwarder's response on a successful forward, and
  re-checks for a `Lookup::Stale` entry only after a forward attempt fails.
  `Pipeline::new` takes `&DnsCacheConfig` and owns the `DnsCache` as an `Arc`.

**Tests (17 new):** 11 cache-module unit tests (TTL expiry + remaining-TTL
math via `tokio::time` paused-clock mocking — same technique as p1-03's
scheduler-jitter test; clamp behavior both directions; RFC 2308 negative
caching with and without an SOA record; RFC 8767 stale-serving and its
window boundary; `serve_stale = false` disables it; `SERVFAIL` never cached;
capacity-bound eviction never exceeded across 500 inserts against a
32-entry cache; an 8-worker real-thread concurrent hammering test against a
64-entry cache, mirroring p1-03's stress-test approach) plus 2 pipeline
integration tests (a second identical query is answered from cache with zero
additional forwarder calls; an upstream failure after the TTL expires serves
the stale entry and flags `QueryEvent.stale`).

**Bench:** `crates/fah-dns/benches/cache.rs` — measures the *whole*
`Pipeline::handle` path for a cache hit (decode → verdict → cache hit →
encode), not an isolated map lookup, since `DnsCache` has no public API to
bench directly and the acceptance criterion is "cache hit adds < 1ms
in-engine latency". Result: **~1.42 µs**, roughly 700x under the 1ms budget.
The bench also asserts the forwarder is called exactly once (during warm-up)
and never again during the timed loop — a regression that made the cache
stop short-circuiting would be caught here, not just silently show up as a
slower number.

**Gates:** fmt/clippy (`-D warnings`)/test all green on the full workspace.
fah-dns: 42 unit + 6 integration tests (was 29 + 6 after p1-04's fixes).

**Deferred, in scope elsewhere:** cache-size gauge for `/metrics` (p1-08 —
`DnsCache::len()` exists today but is `#[cfg(test)]`-only; promote it to a
real accessor when the exporter needs it), prefetch/optimistic refresh
(explicitly out of scope per the task), persistence (RAM-only by design),
**runtime mutability of `[dns.cache]`** (CONFIGURATION.md classes every key
"runtime", but nothing can deliver a config change until p1-09's
`POST /api/v1/config` — apply it there by rebuilding the `DnsCache` and
swapping the `Arc`; losing cache contents on a cache-config change is
acceptable).

**Code review:** docs/code-review/p1-05-review.md — 9 findings, fixes
applied same day (truncated-reply caching, SERVFAIL serve-stale, SOA replay,
hot-path allocations, eviction preference).
