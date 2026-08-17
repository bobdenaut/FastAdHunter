# Code Review — p1-05 DNS Cache

**Scope:** `crates/fah-dns/src/cache.rs` (new), `crates/fah-dns/src/pipeline.rs`
(cache integration), `crates/fah-dns/src/response.rs` (`from_cache`),
`crates/fah-model/src/query_event.rs` (`stale` field),
`crates/fah-dns/benches/cache.rs` (new) ·
**Reviewer:** chief architect pass · **Date:** 2026-07-18 ·
**Status:** findings 1–5 fixed same day + finding 6's deferral recorded (see
"Fixes applied" below); 7–9 are notes with no code change required. Gates
green.

## What was delivered (implementation report)

Per the task's completion note: crate-private `DnsCache` — 16 independently
`Mutex`-locked shards selected by per-instance `RandomState` hash of
`(lowercased domain, qtype, qclass)`; FIFO eviction with stale-first quick
demotion (justified in-code as required); positive TTL = min across answer
set clamped to `[min_ttl, max_ttl]`; RFC 2308 negative caching from SOA
MINIMUM capped by `negative_ttl_max_seconds`; RFC 8767 minimal serve-stale
(24h window, 30s serve TTL, only after a live forward fails, gated on
`serve_stale`); `SERVFAIL`/`REFUSED` never cached. Pipeline checks the cache
on the Allow/Pass path only, after the verdict (ADR-0001 intact — the Block
arm structurally cannot reach the cache); `QueryEvent` gained `stale: bool`.
17 new tests (paused-clock TTL math, clamps, negative w/ and w/o SOA, stale
window boundary, `serve_stale = false`, capacity bound over 500 inserts,
8-worker concurrent hammer) + a whole-`Pipeline::handle` cache-hit bench
(~1.42 µs vs the 1 ms budget) that also asserts the forwarder is never called
during the timed loop.

## Overall assessment

Architecture is right and the hard rules hold: rules-before-cache is
structurally enforced, the cache stores upstream answers only (verdicts can't
reach `store`), memory is bounded and the bound is tested under concurrency,
sharding matches ARCHITECTURE.md §Runtime Model, layering is clean
(`fah-dns` → `fah-config`/`fah-model`; the `QueryEvent` change is a pure data
field). The eviction-choice justification, the `max(min)` clamp-inversion
guard in `new`, per-instance `RandomState` (HashDoS), and the bench asserting
the forwarder stays silent are all exactly the right instincts.

The findings cluster at the seam the cache newly created: what the pipeline
chooses to store and when it chooses to fall back to stale. Two need code
changes; the rest are notes.

## Findings

### 1. MEDIUM-HIGH (correctness) — truncated (TC) upstream replies are cached as negative answers

`UdpForwarder` accepts a reply with the TC bit set (it only checks ID +
`message_type`), and `pipeline::resolve` passes it straight to
`DnsCache::store`. A truncated reply is typically NOERROR with an **empty
answer section** — `store`'s second branch caches exactly that shape as an
RFC 2308 negative entry (no SOA in a truncated reply, so the
`negative_ttl_max_seconds` fallback applies: 60 s by default).

Failure sequence: upstream truncates a large answer over UDP (common at a
1232-byte EDNS ceiling) → we cache "NODATA" → the client retries over TCP as
the TC bit tells it to → our TCP path gets a **fresh cache hit** and answers
NODATA with full confidence. The domain is unresolvable for 60 s, and the
cycle repeats on every expiry as long as queries keep coming. p1-04
explicitly deferred the upstream-TCP-retry to p1-06; caching the truncated
reply converts that known, transient gap into a persistent wrong answer.

**Fix:** `store` returns early when `response.metadata.truncation` is set
(a partial answer set must never be cached, with or without answers). One
test: a TC reply is forwarded to the client but never cached.

### 2. MEDIUM — upstream `SERVFAIL` doesn't trigger serve-stale

RFC 8767's trigger is resolution *failure*, which explicitly includes the
upstream answering SERVFAIL — the most common real-world signature of "my
recursive resolver is having an outage" is a reachable upstream returning
SERVFAIL, not a transport timeout. `resolve` only consults the stale entry
on `forward()` returning `Err`; an `Ok(SERVFAIL)` is relayed to the client
even when a perfectly servable stale entry exists (correctly not cached, but
also not used).

**Fix:** in `resolve`, treat an `Ok` response whose code is `SERVFAIL` as a
failure for stale-fallback purposes: serve the stale entry if one exists,
otherwise relay the upstream's response unchanged. (`REFUSED` is deliberate
upstream policy, not an outage — leave it relayed as-is.) One test: upstream
answers SERVFAIL after expiry → stale entry served, `QueryEvent.stale` set.

### 3. LOW — cached negative replies are replayed without their SOA

`CachedAnswer` keeps only the answer section, so a cached NXDOMAIN/NODATA is
replayed with an empty authority section, while the original upstream reply
carried the SOA that RFC 2308 expects a negative answer to include. LAN stub
resolvers mostly don't care, but anything downstream that does negative
caching of its own (another forwarder, a picky client library) loses its TTL
signal. **Fix (optional, cheap):** store the authority records for negative
entries and replay them with the same TTL stamping `from_cache` already does
for answers.

### 4. LOW (efficiency) — avoidable per-hit allocations on the cache path

Two spots, both inside the <1 ms budget today (~1.42 µs measured) but both
avoidable and PERFORMANCE.md §"no hidden allocations" leans on keeping the
per-query path lean:

- `make_key` runs `to_ascii_lowercase().into_boxed_str()` on **every**
  `lookup`/`store` — and a forward-failure path builds the key up to three
  times. The domain `String` is already owned in `handle`; lowercasing it
  once there (`make_ascii_mut` — in-place, no realloc) and passing it down
  removes one allocation per query and all repeat key builds.
- A `Fresh` hit deep-clones `Vec<Record>` **under the shard lock**. Wrapping
  the stored answer in `Arc<CachedAnswer>` makes the under-lock work a
  refcount bump; `from_cache` already clones per-record to stamp the TTL, so
  nothing downstream changes.

Not blocking; fold into the next touch of `cache.rs` (finding 1 is one).

### 5. LOW — quick-demotion ignores expired entries when `serve_stale` is off

`evict_one` treats only entries past `stale_deadline()` (expiry + 24 h) as
dead weight. With `serve_stale = false`, an entry is unusable the moment its
TTL expires, yet a merely-expired entry is never preferred for eviction over
a still-fresh oldest entry. Wrong entry evicted under capacity pressure —
bounded, correct, just mildly suboptimal. **Fix (optional):** when
`serve_stale` is false, use `expires_at()` as the demotion threshold.

### 6. NOTE (record the deferral) — `[dns.cache]` keys are class "runtime" but nothing can mutate a live cache

CONFIGURATION.md marks every `[dns.cache]` key mutability class **runtime**,
and the task scope says `max_entries` is "runtime-mutable" — but
`DnsCache::new` bakes capacity, clamps and `serve_stale` in at construction
and no resize/reconfigure path exists. This is fine *today* (the config API
is p1-09; nothing can deliver a runtime change yet), and rebuild-and-swap of
the whole `Arc<DnsCache>` at apply time is a legitimate implementation
(losing cache contents on a cache-config change is acceptable). But the
completion note's "Deferred" list doesn't mention it — p1-09 must pick this
up or the config contract is silently broken. Recorded here so it isn't lost.

### 7. NOTE (no change) — negative caching without an SOA deviates from RFC 2308 §5's SHOULD NOT

The RFC says negative answers without an SOA SHOULD NOT be cached;
`negative_ttl` caches them at the `negative_ttl_max_seconds` cap (60 s
default) instead. Deliberate, documented at the decision site, and bounded
small — a defensible pragmatic deviation. No change.

### 8. NOTE (p1-06 pressure) — cache raises the stakes on the forwarder's known anti-spoofing gaps

p1-04 already documents that the temporary `UdpForwarder` neither randomizes
the upstream query ID nor verifies the reply's question section (RFC 5452
§9.1). Now that a cache exists, a single accepted spoof persists for its full
TTL instead of affecting one client once. Both belong to p1-06's real
upstream layer and are already flagged in `forwarder.rs`'s module comment —
this note just upgrades them from "hygiene" to "required before the
temporary forwarder could ever be considered permanent."

### 9. NOTE (no change) — stale-serving applies to negative entries too

An expired NXDOMAIN inside the 24 h window is served stale on upstream
failure. RFC 8767 is written around positive answers, but with upstreams
down every alternative is worse (SERVFAIL storms, retry loops). Fine as is.

## Verdict

The cache itself is well built: bounded and proven so under concurrency,
sharded per the architecture, TTL math tested with a mocked clock at the
boundaries, eviction choice justified where the task demanded it, and the
bench measures the honest end-to-end number rather than a flattering map
lookup. Both real findings live in the pipeline's storage/fallback policy,
not the data structure: it caches one thing it must never cache (truncated
replies — finding 1) and misses the most common real-world trigger for the
serve-stale feature it just built (upstream SERVFAIL — finding 2). Fix both
before p1-06 builds the real upstream layer on this seam; 3–5 are cheap
opportunistic cleanups, 6 and 8 are obligations on p1-09 and p1-06
respectively, recorded here so they don't evaporate.

## Fixes applied (2026-07-18)

Findings 1–5 fixed the same day; 6's deferral recorded; 7–9 need no change.

1. **Truncated replies never cached** (`cache.rs`) — `store` returns early
   when `response.metadata.truncation` is set, with or without answers. New
   test `truncated_reply_is_never_cached` covers both the empty-answer
   (would-have-been-negative) and partial-answer shapes.
2. **`SERVFAIL` triggers serve-stale** (`pipeline.rs`) — `resolve` treats an
   `Ok` upstream response carrying `SERVFAIL` as a resolution failure: the
   stale entry is served if one exists, otherwise the upstream's response is
   relayed unchanged; `REFUSED` stays relayed as deliberate policy. New test
   `upstream_servfail_serves_stale_cache_entry_when_available`.
3. **Negative replies replay their SOA** (`cache.rs`, `response.rs`) —
   `CachedAnswer` gained an `authorities` field, populated from the
   response's authority section for negative entries (empty for positive);
   `from_cache` replays them with the same TTL stamping as answers. New test
   `cached_negative_replay_keeps_its_soa_with_the_stamped_ttl` plus an
   assertion in the existing NXDOMAIN cache test.
4. **Per-hit allocations removed** (`cache.rs`, `pipeline.rs`) — `handle`
   lowercases the domain once in place (`make_ascii_lowercase`, no realloc;
   the matcher is case-insensitive regardless), the cache's new
   `key()`/`lookup(&key)`/`store(&key)` API builds the key once per query
   (`debug_assert` guards the pre-lowercased contract), and `Entry` holds
   `Arc<CachedAnswer>` so a hit clones a refcount under the shard lock
   instead of the record set. The cache-level case-folding test was replaced
   by the pipeline-level `mixed_case_repeat_query_hits_the_cache`.
5. **Eviction respects `serve_stale = false`** (`cache.rs`) — `evict_one`'s
   quick-demotion threshold is the stale deadline with serve-stale on, plain
   TTL expiry with it off. New test
   `evict_one_prefers_expired_entries_when_serve_stale_is_off` asserts both
   behaviors.
6. **Runtime-mutability deferral recorded** — the p1-05 task file's
   completion note now lists `[dns.cache]` runtime mutability as deferred to
   p1-09 (rebuild-and-swap the `Arc<DnsCache>` at config-apply time; losing
   cache contents on a cache-config change is acceptable).

**Verification:** `cargo fmt --check` clean;
`cargo clippy --workspace --all-targets --offline -- -D warnings` clean;
`cargo test --workspace --offline` all green — 177 tests (was 173: +5 new,
−1 replaced), fah-dns 46 unit + 6 integration.
