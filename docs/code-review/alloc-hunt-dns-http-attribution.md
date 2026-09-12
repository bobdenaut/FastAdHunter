# Allocation hunt — DNS and HTTP hot paths, per-operation attribution

Hunt on `main` at `c220956`, 2026-09-12, then a measured fix phase on the
same tree. Same method as
[f3-name-alloc-attribution.md](f3-name-alloc-attribution.md), extended to
the miss path, non-A/AAAA types, `UpstreamPool::forward` and `Proxy::handle`.
The scratch tracer was deleted; raw traces in `target/hunt_dns2.txt`,
`target/hunt_http2.txt` (git-ignored).

Outcome: **implemented H1, H2, H3, D1**; **D4 investigated, recommendation:
do not implement**; H4, D2, D3 recorded as optional, not implemented.

## Summary

- Every allocation on the DNS blocked / hit / miss paths and the HTTP
  blocked / pass-through paths is attributed to one call site (tables below).
- DNS hit path: 12 allocations, 3 owned by FastAdHunter (`domain_of`,
  `CacheKey`, event `Box`); the other 9 are hickory parse, response `Vec`s
  and the `BinEncoder` label-pointer table. Nothing small left to remove there.
- DNS miss path: 17.3 allocations, 7 owned. `DnsCache::insert` clones the key
  twice; one clone is avoidable by passing the key by value.
- Non-A/AAAA queries pay exactly one extra allocation for
  `QueryType::Other(String)`.
- HTTP: 14 of 58 allocations per pass-through request are proxy-owned; 6 of
  them are avoidable with three local changes (`emit` by value, no authority
  intermediate, `retarget` reusing the request's `PathAndQuery`). Blocked
  requests: 11 of 24 owned, 3 avoidable.
- Matcher lookups, `emit_event`, stats and metrics `record`: zero allocations.
- Eight ACTIONABLE fixes proposed; four applied (H1, H2, H3, D1), each with a
  regression ceiling. None touches response bytes, ordering or await points.
  Measured: HTTP pass-through 58→51, blocked 24→20 per request; DNS miss
  17.3→16.3 per handle.
- D4 (`QueryType::Other` allocation) measured against 30 days of live
  traffic: 14.86% of queries would save one 8-byte allocation, ≈1.15% of
  per-query allocations. Technically valid but too low-impact.
- RSS A/B against `c220956`, 8 measured runs plus a control pair on the dev
  box: **allocation reduction confirmed, RSS reduction not observed.** No
  evidence that H1/H2/H3/D1 address the retained-memory / RSS drift.

## Decisions

- Miss-path regression ceilings use `<=` only: cache map and eviction queue
  growth is amortized, so two 64-handle batches differ by ~7 allocations.
- `CacheKey` stays an owned `Box<str>` (INTENTIONAL). Removing the per-hit
  allocation needs a contiguous-bytes key with `Borrow<[u8]>` and stack-built
  lookups (~70 lines) for −1 of 12; a design decision, not a fix.
- `HostResolver::resolve(String)` stays. The only production implementation
  must own the host inside its `'static` future, so `&str` moves the copy
  rather than removing it.
- hickory `BinEncoder::store_label_pointer` (one `Vec<u8>` per label suffix,
  `encoder.rs:275` in 0.26.1) is the largest chunk on every path and is
  record-only; paid twice on a miss (client response + upstream re-encode).
- D4 — technically valid but too low-impact. Recommendation: do not implement
  independently; owner decides. Revisit only if `QueryType` is changed for
  another reason.

## Measurements

Method: `GlobalAlloc` counter over mimalloc plus backtrace capture per
allocation, aggregated by call-site chain. x86 dev box, debug profile (counts
do not depend on opt level), current-thread runtime, 64 warm + 2×64 measured,
then one traced operation. DNS stub forwarder echoes the question and returns
one A record TTL 300; miss = unique name per handle. HTTP: real loopback
origin, proxy and keep-alive hyper client on one runtime; those counts are
whole-process and their ceilings must be re-baselined on hyper upgrades, as
the DNS ones are on hickory.

### DNS — allocations / bytes per `Pipeline::handle`

| Case | Empty answer (F3 harness) | One A record |
| --- | --- | --- |
| blocked inline A `blocked.example.com.` | 13 / 2472 | 13 / 2472 |
| blocked heap A (49 B name) | 19 / 2821 | 19 / 2821 |
| hit inline A `example.org.` | 10 / 1296 | 12 / 2400 |
| hit heap A | 16 / 1665 | 19 / 2834 |
| hit inline AAAA | 10 | 12 |
| hit inline HTTPS | 11 | 13 |
| blocked inline HTTPS | 12 | 12 |
| miss inline A `mNNNN.example.net.` | 14.3 / 1699 | 17.3 / 3020 |
| miss heap A | 19.1 / 2082 | 24.1 / 3442 |

`UpstreamPool::forward`, UDP, loopback stub upstream: 19 / 7912 B
whole-process; ~12 in the pool (4 socket bind + register, 6 request
re-encode, 1 reply buffer 512 B, 1 reply parse), 7 in the harness upstream.

### DNS — attribution, hit inline A, one record (12)

| Site | Count | Bytes | Owner |
| --- | --- | --- | --- |
| `Message::from_vec` `Vec<Query>` | 1 | 88 | hickory |
| `add_query` `Vec<Query>` capacity 4 | 1 | 352 | hickory |
| `add_answer` `Vec<Record>` capacity 4 | 1 | 1088 | hickory |
| `BinEncoder::store_label_pointer` suffix copies | 3 | 12+128+4 | hickory |
| `BinEncoder` `labels_written`, per name emitted | 2 | 16+16 | hickory |
| `Message::to_vec` buffer | 1 | 512 | hickory |
| `domain_of` | 1 | 12 | fah-dns |
| `DnsCache::key` `Box<str>` | 1 | 12 | fah-dns |
| `Event::dns` `Box<QueryEvent>` | 1 | 160 | fah-model |

Heap names (label bytes > 32) add tinyvec spills: parse 2, `from_cache`
`Name` clones 2, `Label` 1. HTTPS vs A: exactly +1 (8 B,
`RecordType::fmt <- to_string <- to_fah_query_type`).

### DNS — attribution, miss inline A, one record (18 traced, 17.3 mean)

| Site | Count | Bytes | Owner |
| --- | --- | --- | --- |
| encoder (3 suffix copies, `name_pointers`, 2 `labels_written`, buffer) | 7 | 18+128+12+4+24+24+512 | hickory |
| `Message::from_vec` `Vec<Query>` | 1 | 88 | hickory |
| upstream reply: `Vec<Query>`, `Vec<Record>` (stub; real reply parses the same) | 2 | 88+1088 | hickory |
| `DnsCache::store` `answers.clone()` | 1 | 272 | fah-dns |
| `DnsCache::insert` `Arc<CachedAnswer>` | 1 | 72 | fah-dns |
| `DnsCache::insert` `CacheKey::clone` (queue + map) | 2 | 18+18 | fah-dns |
| eviction queue / map growth (amortized) | ~1 | 512 | fah-dns |
| `domain_of`, `DnsCache::key`, `Event::dns` | 3 | 18+18+160 | fah |

### HTTP — allocations / bytes per request, whole process

| Case | Total | fah-http-owned | Bytes |
| --- | --- | --- | --- |
| pass-through `GET /resource` | 58 | 14 | 30840 |
| pass-through + `Connection: keep-alive` | 61 | 15 | 30988 |
| blocked script `GET /ad.js` | 24 | 11 | 18426 |
| blocked document (`Accept: text/html`) | 37 | 12 | 20599 |

### HTTP — attribution, fah-http-owned, pass-through (14)

| Site | Count | Bytes |
| --- | --- | --- |
| `Proxy::judge`: host, authority, path, method `to_string`/clone | 4 | 15+15+9+3 |
| `request::absolute_url` | 1 | 31 |
| `Proxy::emit`: `ModelRequest::clone` (host, path, method) | 3 | 15+9+3 |
| `Event::http` `Box<RequestEvent>` | 1 | 192 |
| `claim::destination_of` host `to_string` | 1 | 15 |
| `approved_address`: host clone for the resolver | 1 | 15 |
| `claim::retarget`: path `to_string`; address `to_string` (8, realloc 16) | 3 | 9+8+16 |
| with `Connection` header: `strip_hop_by_hop` `Vec<HeaderName>` | +1 | 128 |

Also caused by fah-http but allocated in dependencies: `Authority` parse copy
in `destination_of` (15 B), two `Bytes` shared headers from `retarget`'s
String-backed URI parts (24 B each), hyper-util `ResponseFuture` box
(2384 B). Blocked requests: judge 5, emit 3, event 1, destination_of 1,
`DecisiveRule` `Arc<str>` 1 (32 B); documents add the explanation page
(512 B).

### Classification

| Id | Site | Class | Reason |
| --- | --- | --- | --- |
| H1 | `proxy.rs` `emit(&Judged)` clones `ModelRequest` | ACTIONABLE | every call site is the last use of `judged` |
| H2 | `proxy.rs:398` authority clone / `format!` | ACTIONABLE | only feeds `absolute_url` |
| H3 | `claim.rs:150,155` `retarget` `to_string` ×2 | ACTIONABLE | `PathAndQuery` is `Bytes`-backed; address string unsized |
| H4 | `proxy.rs:596` `strip_hop_by_hop` collect | ACTIONABLE | `HeaderValue` clone is a refcount |
| D1 | `cache.rs:640,648` `CacheKey::clone` ×2 | ACTIONABLE | key is dead after `store` on the query path |
| D2 | `pipeline.rs:426` `key.clone()` into `swr.offer` | ACTIONABLE | arm returns |
| D3 | `tcp.rs:159` `vec![0u8; len]` per message | ACTIONABLE | buffer reusable per connection; low value |
| D4 | `qtype.rs:19` `QueryType::Other(String)` | ACTIONABLE | `Cow<'static, str>` keeps JSON, `Eq`, `Hash` |
| D5 | `cache.rs:84` `CacheKey` `Box<str>` per lookup | INTENTIONAL | owned map key; representation change, see Decisions |
| D6 | `cache.rs:600` `answers.clone()`, `Arc::new` | INTENTIONAL | response and cache both own records |
| D7 | `udp.rs:157` `to_vec` per datagram | INTENTIONAL | spawned task needs owned bytes; exact size |
| D8 | `plain.rs` socket per forward | INTENTIONAL | random source port (cache poisoning) |
| H5 | three host copies (`Destination`, `ModelRequest`, resolver) | INTENTIONAL | verdict-before-resolve forbids a move; see Decisions |
| H6 | `block.rs:71` explanation page | INTENTIONAL | body must be owned |
| R1 | `matcher.rs:1105,1132` `DecisiveRule` `Arc<str>` | INTENTIONAL | lazy materialization by design (F3) |
| — | `Event` boxes (160 / 192 B) | INTENTIONAL | 16-byte channel item by design |
| — | hickory encoder label table, parse `Vec`s, `Name` spills | UPSTREAM | inside `hickory-proto` 0.26.1 |
| — | `add_query`/`add_answer` capacity-4 `Vec`s | RECORD-ONLY | bytes only (F3 decision) |
| — | `plain.rs:74` `vec![0; reply_budget]` | RECORD-ONLY | stack array bloats the future by up to 4 KB |
| — | hyper-util `ResponseFuture` box, `Authority` parse copy | UPSTREAM | http / hyper-util |
| — | matcher lookups, `context_for`, `emit_event`, stats, metrics | NO MATERIAL ISSUE | zero allocations in every trace |

### Proposed fixes (not applied)

| Id | Change | Expected | Regression test |
| --- | --- | --- | --- |
| H1 | `fn emit(&self, judged: Judged, …)`; move `request`, `verdict`, `policy`; five callers pass `judged` | −3 per HTTP request, all paths | new `crates/fah-http/tests/proxy_alloc.rs`, whole-process ceilings 58→55, 24→21, 37→34 |
| H3 | `uri.path_and_query().cloned().unwrap_or_else(\|\| PathAndQuery::from_static("/"))`; authority `String::with_capacity(47)` + `write!` | −2 to −3 per forwarded request | same file; existing `retarget` unit tests |
| H2 | `absolute_url(request, host, port: Option<u16>)` writes scheme, host, optional `:port`, path into one pre-sized `String` | −1 per request | same file; existing `absolute_url` unit tests |
| D1 | `store(&self, key: CacheKey, …)`; `insert` moves into map, clones once for queue; `pipeline.rs` passes `key`; `swr.rs:200` passes `key.clone()` (line 211 still needs it) | −1 per miss | `forward_alloc.rs` miss case, `<=` 17 per handle (before 1110 / 64 fails, after ~1046 passes) |
| D4 | `QueryType::Other(Cow<'static, str>)`; `to_fah_query_type` maps the 8 stats-tracked types to `Cow::Borrowed`, fallback `Cow::Owned`; `wire.rs:255,265`, `bucket.rs:35`, `matcher.rs:219` adapt | −1 per non-A/AAAA query | `forward_alloc.rs` HTTPS hit case, ceiling 12 |
| H4 | collect `Connection` values into `[Option<HeaderValue>; 4]` (refcount clones), `Vec` fallback past 4, then remove names | −1 per message with `Connection` | keep-alive case ceiling 61→60 |
| D2 | `swr.offer(&self.cache, key)` | −1 per SWR stale serve | none; pure move |
| D3 | hoist `Vec<u8>` out of the TCP loop, `resize(len, 0)` | −1 per TCP query after the first | none proposed |

### Fix phase — before → after (H1, H2, H3, D1 applied together)

HTTP, `crates/fah-http/tests/proxy_alloc.rs`, whole process per request:

| Case | Allocations | Bytes | Ceiling |
| --- | --- | --- | --- |
| pass-through `GET /resource` | 58 → 51 | 30840 → 30805 | 51 |
| pass-through + `Connection: keep-alive` | 61 → 53 | 30988 → 30943 | 53 |
| blocked script | 24 → 20 | 18426 → 18401 | 20 |
| blocked document | 37 → 32 | 20599 → 20570 | 32 |

DNS, `crates/fah-dns/tests/forward_alloc.rs`, per `handle`, two 64-handle
batches:

| Case | Allocations | Bytes | Ceiling |
| --- | --- | --- | --- |
| miss inline | 17.5 / 17.3 → 16.5 / 16.3 | ~3020 → 2957 / 2924 | 17 (`<=` only) |
| miss heap | 24.2 / 24.1 → 23.1 / 23.1 | ~3442 → 3286 / 3371 | 24 (`<=` only) |
| blocked 13 / 19, hit 10 / 16 | unchanged | | unchanged |

Pre-fix values fail every new ceiling (1120 > 1088, 1549 > 1536, 58 > 51).
Miss-path bytes swing ±60 B per handle between batches from amortized
map/queue growth; the −1 allocation is exact. Gates: fmt, clippy
`-D warnings`, full workspace tests green.

### RSS A/B — B (`3287418`, fixes) vs A (`c220956`, before)

Question: does the reduced allocation churn move process RSS or retained
memory? Not a soak — one short controlled experiment, no source or allocator
change.

| Held identical | Value |
| --- | --- |
| build | release profile, same toolchain; A from a worktree at `c220956` |
| config | one file copied per run; `dns.cache` defaults, `runtime.http_runtimes = 2` |
| upstream / origin | Python stub upstream (one A record, TTL 300) + stub loopback origin on :80 |
| per run | fresh process, fresh `/config` + `/data`; 20 s settle → warmup 30k DNS + 3k HTTP → measured window → 120 s quiet |
| workload | 300,000 DNS @ 2000.0 qps (210,000 unique names = misses, 90,000 over a 64-name hit pool) + 30,000 pass-through keep-alive `GET` @ 200.0 rps, 8 connections |
| order | interleaved A/B/A/B/A/B/A/B, then one control pair |

Every run: 100% DNS replies, 30,000/30,000 HTTP `200`, zero drops, and
`cache_entries = 10000` at the end — saturated compared against saturated
([measurement-traps.md](../measurement-traps.md) §Memory). RSS is
`WorkingSetSize` at 2 Hz via `GetProcessMemoryInfo`; `/debug/memory`
`process_rss` is `/proc`-only and reads `null` here.

| Run | Boot | Steady pre-load | Peak | Post | Residual | Slope, final third |
| --- | --- | --- | --- | --- | --- | --- |
| A1 | 32.09 | 46.89 | 54.95 | 54.65 | 55.12 | +0.22 |
| A2 | 19.84 | 34.21 | 45.29 | 44.62 | 45.01 | +0.52 |
| A3 | 20.24 | 35.57 | 44.25 | 44.00 | 44.77 | +0.54 |
| A4 | 20.14 | 35.94 | 44.45 | 43.58 | 44.20 | +0.32 |
| B1 | 20.28 | 34.99 | 44.78 | 44.24 | 44.66 | +0.20 |
| B2 | 19.86 | 34.49 | 43.41 | 43.02 | 43.43 | +0.27 |
| B3 | 19.98 | 36.08 | 43.02 | 42.21 | 42.92 | +0.36 |
| B4 | 20.36 | 35.29 | 43.88 | 43.85 | 44.22 | +0.26 |

MB. A1 is excluded from the means: boot RSS 32.09 against ~20.0 everywhere
else — a different starting state (first process of the session, right after
two concurrent cargo builds). Including it fakes a −3.5 MB win.

| Metric | A mean (n=3) | B mean (n=4) | Delta B−A | A range | B range |
| --- | --- | --- | --- | --- | --- |
| steady pre-load | 35.24 | 35.21 | −0.03 (−0.1%) | 34.21–35.94 | 34.49–36.08 |
| peak | 44.66 | 43.77 | −0.89 (−2.0%) | 44.25–45.29 | 43.02–44.78 |
| post-workload | 44.07 | 43.33 | −0.74 (−1.7%) | 43.58–44.62 | 42.21–44.24 |
| residual, +120 s | 44.66 | 43.81 | −0.85 (−1.9%) | 44.20–45.01 | 42.92–44.66 |

Control arm — hit-only DNS, 300,000 queries, no HTTP, a path the fixes do not
touch: A 34.93 / 35.06 / 34.08, B 35.46 / 35.66 / 34.72 → B−A **+0.53 /
+0.60 / +0.64** on steady / peak / residual. Unchanged code reads B *higher*
by the same order the measured arms read it *lower*, every range overlaps,
and both sit far inside mimalloc's ±6 MB purge band.

Allocation correlate over one measured window (per-op from each arm's own
counter tests; A's miss and proxy cases are the pre-fix column above — those
tests land with the fix, so A's tree has no equivalent):

| Operation | A | B | Per window |
| --- | --- | --- | --- |
| DNS miss, inline name | 17.5 / 17.3 | 16.48 / 16.31 | 210,000 misses → −210,000 allocations, ≈−12.6 MB allocated |
| HTTP pass-through + keep-alive | 61 | 53 | 30,000 requests → −240,000 allocations, ≈−1.35 MB allocated |
| DNS cache hit, inline | 10 | 10 | unchanged |

≈450,000 fewer allocations and ≈14 MB less allocated per run, and RSS does
not move. `/debug/memory` `accounted_bytes` is identical arm to arm — 6.14–
6.16 MB steady, 7.65–7.70 MB post-load — which is the mechanism: the removed
allocations are transient, freed inside the operation, and never reach
retained memory. `allocator_committed_bytes` ran 79–109 MB with no arm
pattern (lifetime high-water, not footprint). Residual slope was +0.20 to
+0.54 MB in every run, both arms — no leak signal, no arm difference, and no
leak is claimed from RSS either way.

**Finding: allocation reduction confirmed, RSS reduction not observed. No
evidence that H1/H2/H3/D1 address the retained-memory / RSS drift.**

Scope: x86 Windows dev box, mimalloc v3, ~5.5-minute runs, stub upstream and
stub origin, 2000 qps / 200 rps. Says nothing about RB5009 / musl / multi-day
retention; superseded only by an on-device measurement, not another dev-box
run.

### D4 — live traffic share of non-A/AAAA types

`GET /api/v1/history/summary`, RB5009 live resolver, 30 day-files
(retention cap), 2026-08-14 00:00Z → 2026-09-12 06:00Z, 691 hourly points,
stride 1. Per-type sums equal the query totals exactly.

| | Count | Share |
| --- | --- | --- |
| total queries | 2,527,201 | 100% |
| blocked | 1,734,430 | 68.6% |
| cache hits | 706,912 | 28.0% |
| A | 1,128,618 | 44.66% |
| AAAA | 1,023,017 | 40.48% |
| HTTPS | 365,484 | 14.46% |
| SRV | 3,280 | 0.13% |
| OTHER (untracked types) | 2,788 | 0.11% |
| PTR | 1,410 | 0.06% |
| SOA | 1,265 | 0.05% |
| NS | 777 | 0.03% |
| CNAME | 192 | 0.01% |
| TXT | 186 | 0.01% |
| MX | 184 | 0.01% |
| **non-A/AAAA** | **375,566** | **14.86%** |

Per-day non-A/AAAA share 6.9%–25.0%; no day under 18k queries; ~84k
queries/day. Window judged sufficient.

Impact of D4: one 8-byte allocation per non-A/AAAA query. Fleet-weighted
(68.6% blocked at 13, 28% hits at 12, ~3.4% misses at 17–24 → ~12.9
allocations/query): −0.149 per query, ≈1.15% of per-query allocations,
~12.5k allocations/day. Cost: model type change in fah-model plus touches in
fah-api, fah-stats, fah-rules, fah-dns. Verdict in Decisions.

### Status

| Id | Status |
| --- | --- |
| H1, H3, H2, D1 | implemented, ceilings added |
| RSS A/B | run; allocation reduction confirmed, no RSS effect observed (dev box) |
| D4 | investigated; recommend not implementing — technically valid but too low-impact; owner decides |
| H4, D2, D3 | optional, low value, not implemented |

## Files changed

- `crates/fah-http/src/proxy.rs` — `emit(Judged)` by value; `judge` passes
  host and optional port, no authority String.
- `crates/fah-http/src/claim.rs` — `retarget` clones the request's
  `PathAndQuery`, pre-sizes the address String (`SOCKET_ADDR_TEXT_MAX`).
- `crates/fah-http/src/request.rs` — `absolute_url(request, host, port)`.
- `crates/fah-dns/src/cache.rs` — `store`/`insert` take `CacheKey` by value;
  map insert via `hash_map::Entry` so replaced-entry byte accounting keeps
  the key.
- `crates/fah-dns/src/pipeline.rs` — moves `key` into `store`.
- `crates/fah-dns/src/swr.rs` — `key.clone()` into `store` (failure path
  still needs the key; background, count unchanged).
- `crates/fah-http/tests/proxy_alloc.rs` — new, four whole-process ceilings.
- `crates/fah-dns/tests/forward_alloc.rs` — miss ceilings (echoing stub +
  A record, unique names), bytes counter.

## Remaining TODOs

- H4, D2, D3 stay optional; none scheduled.
- Retained-memory / RSS drift stays open: nothing here explains it, and the
  A/B is dev-box only. Re-measure on the RB5009 before it generalizes.
- Raise the `store_label_pointer` suffix copy with hickory upstream (store
  offsets, compare against the buffer).
