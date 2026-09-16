# Hot-path audit — DNS and HTTP entry points

Read-only audit. No file outside this one was changed. Every count below is
**production-only**: the `#[cfg(test)]` tail of each file was cut mechanically
at the first `#[cfg(test)]` line before counting, except `cache.rs`, where the
first two markers guard test-only accessors (`675`, `685`) and the real module
boundary is `862`.

Device: dev box, x86-64, `rustc 1.96.0`, debug profile. Corpus and workload are
named per measurement. Convert to RB5009 with the ~9× factor, never from clock
readings.

## Summary

- Scope: `Pipeline::handle`, the UDP/TCP/DoT accept loops, `Proxy::serve_connection`
  / `judge` / `emit`, `sni.rs` and the HTTPS accept path, plus every callee one
  level deep — 19 files in two crates and three outside them.
- Ten findings, none critical, none blocking. Eight are low or informational.
- The two that matter: the ClientHello reader rescans its whole buffer after
  every socket read, on one of only two threads carrying all proxy traffic; and
  a poisoned cache shard kills every query hashing to it, forever and silently.
- **All ten were cross-checked against API.md, ARCHITECTURE.md,
  CONFIGURATION.md, CONTEXT.md, PERFORMANCE.md, RULE_ENGINE.md and
  SECURITY.md.** No finding contradicts a documented decision. Two got worse
  under that check (2 and 9), one gained a budget row to aim at (3), and one
  sits in a gap the docs never covered (1).
- **The hot path is not allocation-free, and PERFORMANCE.md's invariant stands**
  (Finding 10). 10-23 allocations per DNS query, 50-53 per HTTP request, plus
  what no oracle covers. That is performance debt against a deliberate project
  invariant, not a doc that needs softening. §Measurements is the debt register.
- Every allocation oracle passes, but each ceiling is pinned exactly at the
  observed count. Real margin is the 4-allocation jitter allowance spread over
  64 iterations — 0.0625 per query. That is by design and it is also brittle.
- The oracles only ever send `RecordType::A`, so one allocation per HTTPS /
  SVCB / TXT / PTR query is invisible to them.
- No `unsafe` anywhere in scope. No `std::sync` guard is held across an `.await`.
- F1 looks closed: the UDP reply failure path now goes through `LogThrottle`.
  F2 is the documented cache shard `Mutex` — cited, not re-reported. F3 is
  closed at `tcp.rs:223` — measured, no code change. F4 is not a code matter.

## Decisions

- **F2 is cited and closed out.** The shard-selected `Mutex` in `cache.rs` is
  argued in the file header and in ARCHITECTURE.md §Runtime Model. This audit
  does not reopen it. Finding 1 is about the `.unwrap()` on the guard, not
  about the lock existing.
- **No bench claim is made.** Seven benches have an own-side spread wider than
  the 10% gate (F9), and nothing here was measured against a pre-change
  checkout. Making no claim is the honest outcome.
- **The surviving per-query allocations are debt, not scope.** They are listed
  individually in §Measurements and totalled in Finding 10. A row marked
  "structural" or "required" there carries an argument, not a measurement —
  cloning `Record`s out of the cache needs a pre-encoded wire cache to remove,
  and the two heap copies of the domain name (`qtype.rs:27`, `cache.rs:485`)
  need `CacheKey` to share an `Arc<str>` with the event. Both are design work
  behind a measurement, and neither is settled. The invariant does not move to
  meet them.
- **`http_runtimes = 2` is what sets Finding 2's severity, not the CPU cost.**
  A per-connection cost that would be minor across four cores is a
  browsing-availability lever across two shared threads. The default, not the
  algorithm, is the reason that finding is HIGH.

## Findings

Severity-ranked.

### 1 — MEDIUM · a poisoned cache shard permanently kills every query on it

`crates/fah-dns/src/cache.rs:519`, and the same construct at `555`, `566`,
`626`, `719`, `778`:

```rust
let mut guard = shard.lock().unwrap();
```

**Rule broken:** engineering principle 1 (correctness before optimization) and
hard rule 4's spirit — a resolver that answers nothing is not bounded
degradation.

**Failure scenario.** `crates/fah-dns/src/pipeline.rs:245-246` deliberately
survives a panicked sweep:

```rust
Err(err) => {
    tracing::warn!(error = %err, "cache cleanup sweep failed");
    continue;
}
```

The comment above it says "the scheduler outliving one bad sweep is worth more
than the entries it would have removed; the next tick retries." But `clean`
holds `shard.lock()` across its whole `retain` walk (`cache.rs:778`). A panic
anywhere inside that walk poisons the shard. From then on every
`lookup` / `store` / `stats` call on that shard hits `.unwrap()` on a
`PoisonError` and panics. In the query path that panic lands in the
per-datagram `tokio::spawn`, which tokio catches — so the client gets no reply
at all, silently, forever, for every name that hashes to that shard. The
scheduler the comment tried to protect is still alive and still logging
"cleanup complete" every interval.

The same applies to a panic under `insert`'s eviction loop, and to the admin
`POST /api/v1/cache/clean`.

**The supervision model cannot see it either.** ARCHITECTURE.md §Runtime Model:
"Long-lived task death is observed, not handled. The binary's run loop checks
every supervised task on a 10 s tick; one that ended before shutdown is logged
once and counted in `counters.tasks_died`." That mechanism keys on the task
*ending*. The sweep deliberately does not end — it logs and `continue`s — so a
panicked sweep never increments `tasks_died`, and the shard it poisoned is
reported nowhere. The one signal an operator would have is the absence of
answers for a sixteenth of the names they ask for.

**No document covers this.** ARCHITECTURE.md §Runtime Model, CONFIGURATION.md
§The cleanup sweep is not a third bound, and PERFORMANCE.md §Design costs worth
knowing all describe the sweep's shard-at-a-time locking, and none of them
mentions poisoning. The fix closes a gap rather than contradicting a decision,
so it needs no ADR.

**How likely.** Low in a release build: `debug_assert!` is compiled out, and
integer overflow does not panic. Allocation failure aborts rather than unwinds.
So this is latent, not live. It is still the difference between the stated
intent and what the code does.

**Smallest fix.** Six call sites, one construct:

```rust
use std::sync::PoisonError;

let mut guard = shard.lock().unwrap_or_else(PoisonError::into_inner);
```

**API proof.** `library/std/src/sync/poison.rs:313` in the 1.96.0 source tree
shipped with this toolchain:

```rust
#[stable(feature = "sync_poison", since = "1.2.0")]
pub fn into_inner(self) -> T {
```

The doc example directly above it (lines 300-311) recovers a map after a thread
panicked mid-`insert`. The repository already uses exactly this idiom at
`crates/fah-dns/tests/forward_alloc.rs:106`.

**Cost.** Memory: none. Hot path: none — `unwrap_or_else` on the `Ok` arm is
the same branch `unwrap` already takes. Build time: none. Complexity: one
import.

**What goes wrong if it is not fixed.** One panic under the shard lock turns
1/`SHARD_COUNT` of the cache keyspace into a silent black hole for the life of
the process, with no log line pointing at it.

**What test would fail if this were already safe?** None exists. The nearest
thing to write: poison a shard from a panicking scope, then assert `lookup`
still returns `Lookup::Miss` rather than panicking.

### 2 — CLOSED, no code change · `read_client_hello` rescans the whole buffer after every read

**Measured on the RB5009 and closed, 2026-09-16 — the amplification does not
bite.** A diag-timing probe on veth3 (`fah-diagprobe`, http_runtimes=2, the
production default) was driven with a **corrected** adversarial ClientHello: a
single ClientHello fragmented into ~4051 tiny handshake records, declaring a
large non-SNI extension whose body never arrives, so `scan_client_hello`
genuinely stays `Incomplete` and re-walks every accumulated record on each read
— the O(records²) path this finding names. The first attempt was a bad
stimulus that resolved to `NoSni` after ~40 bytes; the corrected one was
verified to engage the real path (`listeners.https.hello_timeouts` 0 → 12 when
the drippers were held to the 10 s timeout).

Ladder of concurrent drippers against normal HTTPS-peek and HTTP-proxy traffic
on the two shared allocation domains, dev-box client, pace 0.6 ms:

| drippers | HTTPS p50 | HTTPS p95 | HTTP p50 | HTTP p95 | conns Δ |
| -------: | --------: | --------: | -------: | -------: | ------: |
| 0 | 16.1 | 30.0 | 16.1 | 24.2 | 30 |
| 2 | 16.2 | 31.1 | 16.5 | 31.2 | 34 |
| 4 | 17.0 | 30.5 | 16.4 | 29.6 | 38 |
| 8 | 14.5 | 29.9 | 17.9 | 29.9 | 51 |
| 16 | 14.5 | 29.2 | 15.1 | 28.4 | 78 |
| 32 | 5.8 | 26.0 | 5.6 | 25.6 | 126 |

**p95 is flat from 0 to 32 drippers — 16× the two domain threads — for both
HTTPS and HTTP.** The 2-dripper row (exactly one per domain, the case
http_runtimes=2 makes worst) is indistinguishable from baseline. `conns Δ`
climbs 30 → 126 as the drippers reconnect on the 16 KiB cap, so the load
genuinely reached `read_client_hello` at every step. The p50 drop at 32 is
client-side scheduling on the single dev-box driver, not a server effect; p95
is the load-bearing number and it does not move.

**Why it does not bite:** `MAX_HELLO_BYTES = 16 KiB` caps records at ~2730, so
the O(records²) rescan is ~7 M trivial integer steps per connection spread over
at most the 10 s `hello_timeout`, and `[https] max_connections` bounds how many
such connections can exist at once. The three limits together neutralise the
amplification on the RB5009. **The residual lever is raw connection count, which
is a different bound than rescan amplification and is exactly what
`max_connections` exists to hold.**

CPU was not sampled (it needs a RouterOS read, off limits here); the
latency-interference measurement answers the severity question on its own —
the domains were never starved.

**Disposition:** closed without a code change. `scan_client_hello` is left
exactly as it was. Corpus/workload/device: adversarial 4051-record ClientHello,
0.6 ms pace, up to 32 concurrent drippers, `fah-diagprobe` on RB5009 veth3,
http_runtimes=2, 2026-09-16. Superseded only by a workload that moves p95 — a
sustained many-thousand-connection flood would, but that is the
`max_connections` bound's problem, not the parser's.

--- the read-only audit's original argument, kept for the record ---

`crates/fah-http/src/https.rs:394-412`:

```rust
let read = stream.read_buf(&mut hello.limit(want)).await?;
if read == 0 { return Err(io::Error::from(io::ErrorKind::UnexpectedEof)); }
match scan_client_hello(hello) {
    HelloScan::Incomplete => {}
    scan => return Ok(scan),
}
```

**Rule broken:** hard rule 4 (bounded everything) in its CPU sense, and the
§5 rule that "nothing" on a path a hostile client drives is a finding.

**Raised from MEDIUM on the documentation cross-check.** Three documents put
this in a much smaller blast radius than "one core of four":

- **SECURITY.md §Threat model** already treats the attacker as present: "The
  LAN is **not** trusted: any compromised IoT device can sniff traffic or
  ARP-spoof." The earlier wording here, "an unauthenticated remote", understated
  it. The accurate phrase is **any LAN device**, and the threat model assumes
  one is hostile.
- **ARCHITECTURE.md §HTTPS SNI**: "the ClientHello peek, the SNI verdict, the
  splice or the MITM handshake and the session all run on the domain thread,
  never on the acceptor."
- **CONFIGURATION.md:81**: `http_runtimes` defaults to `max(1, cores/2)` — **2
  on the RB5009** — and HTTP and HTTPS hand their sockets to the *same*
  allocation domains.

So the rescan does not burn one of four cores. It burns **one of two threads
carrying all HTTP and HTTPS traffic**. Two slow-drip clients occupy both for
`hello_timeout` and the household's browsing stalls.

**What caps it.** DNS is unaffected: the resolver runs the full pipeline on the
shared runtime's workers, not on the allocation domains (ARCHITECTURE.md
§Runtime Model). Name resolution keeps working while the proxy is stalled.
That is the difference between this and a BLOCKED verdict.

**Failure scenario.** `scan_client_hello` restarts from byte 0 on every call.
A client that dribbles the ClientHello one TCP segment at a time forces one
full re-walk per segment. The walk is O(TLS records + extensions), not O(bytes),
because `Records::skip` jumps a whole record at a time — so a normal client
pays nothing. A client that frames 16 KiB as ~3270 one-byte records and sends
them one segment at a time pays roughly 16384 reads × up to 3270 record steps ≈
5×10⁷ pointer steps per connection, all inside `hello_timeout`, all before any
authentication. Multiply by `https.max_connections`.

**What bounds it today.** `MAX_HELLO_BYTES = 16 * 1024` caps the buffer,
`hello_timeout` caps the wall clock, and `max_connections` caps concurrency.
So this is amplification inside an existing bound, not an unbounded hole.
Nothing bounds the work *per byte received*, which is the part an attacker
controls, and `http_runtimes = 2` is what makes the amplification bite.

**Smallest fix, and the honest answer.** The cheap version is to remember how
many bytes were scanned last time and skip the rescan until the buffer has
grown past the point the previous walk stopped at — but `Records` does not
currently report where it stopped, so that is a real change to the scanner's
shape. A simpler partial fix is to skip the scan whenever `read` was small and
the buffer does not yet end on a record boundary.

Both add state to a parser that is currently clean. **This is still measure-first**
(principle 8), but it is now **first in that queue, ahead of Finding 3.** The
measurement: drive the HTTPS listener with a one-byte-record ClientHello for the
full `hello_timeout` and record CPU against a normal handshake, on the RB5009,
with `http_runtimes` at its default of 2. Watch whether a second concurrent
dripper stalls unrelated proxy traffic — that, not the CPU ratio alone, is the
question the two-domain default raises.

**What goes wrong if it is not fixed.** Two cheap clients can hold both
allocation-domain threads for `hello_timeout` at a time, stalling all HTTP and
HTTPS proxying. Not a crash, not unbounded, and not a DNS outage — a
browsing-availability lever that SECURITY.md's own threat model says is within
reach of a compromised LAN device.

### 3 — CLOSED, no code change · `spawn_blocking` on every DoT connection, including cache hits

**Measured on the RB5009 and closed, 2026-09-16 — the round trip is real but
not worth removing.** The diag-timing probe on veth3 was driven through 6 cold
first-sight DoT handshakes (each mints a leaf) and 19 warm handshakes to one
repeated host (leaf cached — `leaf_cache.prewarm_hits` went 0 → 19, `minted`
0 → 6, so the warm cohort is exactly the cache-hit path this finding names).

Per-stage timing from the container's `DoT first-sight timing` line, µs:

| stage | cold (n=6) | warm (n=19) | what it is |
| ----- | ---------: | ----------: | ---------- |
| `dispatch_wait_us` | ~65 | **~62** | the `spawn_blocking` round trip — this finding's target |
| `prewarm_us` | ~2008 | ~9 | leaf mint (cold) versus cache lookup (warm) |
| `handshake_after_prewarm_us` | ~4700 | ~4700 | TLS crypto + LAN RTT, client-contaminated |
| `sni_to_dispatch_us` | ~0 | ~0 | SNI parse |

**The `spawn_blocking` overhead is `dispatch_wait_us` ≈ 62 µs, stable across
the whole sample (cold and warm alike).** Against PERFORMANCE.md's recorded
1.389 ms DoT budget miss that is **~4.5 %** — measurable, not material. The miss
is dominated by the cold mint (`prewarm_us` ≈ 2.0 ms here, itself 4× the 450 µs
`certs_mint` budget — a CPU-regime effect, not this finding) and by handshake
completion. And `spawn_blocking` moves the work **off** the domain thread onto
the blocking pool, so the task awaits rather than burning a DNS worker: the
"displacing query tasks" concern is minimal.

**The fix was considered and rejected on this measurement, not deferred.**
Adding a `CertStore` accessor that runs the leaf-cache fast path inline on the
async thread — guarded by `leaf.rs`'s `std::sync::Mutex` touched from an async
worker — would save ~62 µs on a warm connection at the cost of new
synchronization on a hot path. Principles 8 and 16: an optimisation must be
justified by a measurement, and ~62 µs against a millisecond-scale miss does not
justify a new lock reachable from async. `serve_connection` and `prewarm` are
left exactly as they are.

**Disposition:** closed without a code change. Corpus/workload/device: 6 cold +
19 warm DoT handshakes, `fah-diagprobe` on RB5009 veth3, CA present, 2026-09-16.
Revisit only if DoT becomes latency-critical (Android Private DNS at scale) —
the trigger and the invalid alternative are unchanged from A3 in the earlier
audit.

--- the read-only audit's original argument, kept for the record ---

`crates/fah-dns/src/dot.rs:166-190`:

```rust
let joined = tokio::task::spawn_blocking(move || store.prewarm(&host)).await;
```

**Rule broken:** engineering principle 2 (keep the hot path small) — this is a
per-connection cost, not per-query.

**Failure scenario.** `CertStore::prewarm` (`crates/fah-certs/src/store.rs:431`)
calls into `leaf.rs:155`, whose first act is `self.lease(host, now)`. On a warm
leaf that returns `Lease::Fresh(key)` after one `Mutex` acquire and a map
lookup — microseconds. Every DoT connection to an already-minted host still
pays a full blocking-pool round trip: task allocation, channel send, worker
thread wakeup, join. On the RB5009's four cores that is scheduler work
displacing DNS query tasks, for a result the code then throws away
(`Ok(Ok(_)) => {}`) because the `MintingResolver` fetches the leaf again during
the handshake.

**Also a cancellation hazard.** If the connection task is aborted at exactly
this `.await` — which shutdown does — the blocking closure keeps running
detached. It cannot be cancelled. Bounded (one per connection, and it holds
only an `Arc<CertStore>`), but it is state left half-done at a cancellation
point and belongs on the record.

**There is already a budget row this would move.** PERFORMANCE.md §Budgets puts
cold `prewarm` at **450.88 µs against a < 1 ms budget** — comfortably inside —
and then records that the end-to-end DoT figure is not: "P5's end-to-end DoT
increment misses the same budget at 1.389 ms: it tracks the CPU speed regime,
not the mint, and is a recorded budget miss rather than a defect."

So the mint is fast and the end-to-end number is ~3× it, with the gap currently
attributed to the CPU speed regime. **A blocking-pool round trip on every
connection, warm or cold, is a second candidate for part of that gap** — one
this audit can name and the existing attribution did not consider. That does
not overturn the recorded explanation; it means the miss has two plausible
contributors and only one has been tested.

This gives the deferred measurement both a hypothesis and a row it would move,
which is more than "it looks wasteful".

**Smallest fix.** Try the cache on the async thread first, dispatch to
`spawn_blocking` only on a miss. That needs a new non-blocking accessor on
`CertStore` — something like `fn cached(&self, host: &str) -> Option<Arc<CertifiedKey>>`
that runs `lease`'s fresh path and gives up rather than minting. The leaf cache
already has the shape for it (`leaf.rs:173-190`).

**Cost.** Memory: none. Hot path: removes a blocking dispatch per warm DoT
connection. Complexity: one new method on `CertStore`, and a caveat — the fast
path takes `leaf.rs`'s `self.lock()`, a `std::sync::Mutex`, on an async thread.
That lock is only ever held for a map lookup, so the risk is small, but it is a
new place where an async worker can block.

**Not worth doing blind.** Measure the warm-connection handshake latency with
and without the dispatch first, on the RB5009, with `diag-timing` — the feature
already exists at `dot.rs:192-237` and reports `dispatch_wait_us` separately
from `prewarm_us`. If `dispatch_wait_us` is already near zero under load, skip
this, and the 1.389 ms miss stays attributed to the CPU speed regime alone.
`diag-timing` is exactly the instrument that settles it, because it splits
`sni_to_dispatch_us`, `dispatch_wait_us`, `prewarm_us` and
`handshake_after_prewarm_us` into four numbers rather than one.

### 4 — LOW · a removable `CacheKey` clone on the stale-serve path

`crates/fah-dns/src/pipeline.rs:459`:

```rust
swr.offer(&self.cache, key.clone());
```

**Rule broken:** hard rule 3 (no allocations on the hot path) and principle 3.

`CacheKey::domain` is a `Box<str>`, so this is one heap allocation and one
copy of the domain name, on every stale hit that wins its refresh claim.

`key` is not needed after this point: the branch returns three lines later at
`pipeline.rs:466`. A conditional move followed by a `return` in the same branch
is accepted by NLL, so this compiles:

```rust
swr.offer(&self.cache, key);
```

`SwrPool::offer` already takes `key: CacheKey` by value (`swr.rs:113`), so no
signature changes.

**Cost.** Memory: one fewer allocation per claimed stale hit. Hot path:
strictly less work. Complexity: negative.

**What goes wrong if it is not fixed.** Nothing breaks. It is one avoidable
allocation on a path ADR-0005 exists to make cheap.

**What test covers it?** None. `forward_alloc` measures fresh hits, blocks and
misses, never a stale hit with SWR enabled. Adding that case would make this
finding measurable instead of argued.

### 5 — LOW · unthrottled `error!` per connection after every HTTP domain dies

`crates/fah-http/src/server.rs:330`:

```rust
tracing::error!("no HTTP domain left; connection dropped");
```

**Rule broken:** §5 — an error path an unauthenticated remote can drive with no
rate limit of any kind.

**Failure scenario.** `Rotation::send` removes a domain from the rotation each
time its channel is closed (`server.rs:323`). Once the last one is gone, every
subsequent accepted connection emits one `error!` line. At `error` level this
is never filtered — unlike the `debug!` sites in the same file — so a client
looping connect/disconnect writes one error line per connect, indefinitely.
On the RB5009's log buffer that costs real history, which is exactly what the
cache-sweep comment at `pipeline.rs:255-262` already argues about at `info`.

**What rate-limits it today.** Nothing. The sibling paths in this codebase all
use `LogThrottle` (`udp.rs:38`, `tcp.rs:42-43`, `pipeline.rs:159`,
`server.rs:210`).

**Smallest fix.** The same `LogThrottle` the accept loop above it already
holds, or — simpler — collapse the repeat case to `debug!` and keep one `error!`
for the transition, emitted where the last domain is removed at
`server.rs:322-324` rather than per connection.

**Cost.** Memory: one `LogThrottle` (24 bytes) if the throttle route is taken,
zero if the transition route is. Hot path: none, this is a failure path.
Complexity: low either way. The transition version is simpler and is presented
first.

**What goes wrong if it is not fixed.** After a process-level failure the logs
that would explain it are pushed out of the buffer by the failure's own
aftermath.

### 6 — LOW · two heap copies of the SNI host per DoT connection

`crates/fah-dns/src/dot.rs:136` and `dot.rs:173`:

```rust
Some(host) if tls.store.has_ca() => Some(host.to_owned()),
...
let logged = host.clone();
```

**Rule broken:** principle 3 (avoid heap allocations; every one needs a
justification).

The first is forced: `spawn_blocking` needs `'static`, so the borrow from
`start.client_hello()` cannot cross it. The second exists only so the host can
be named in a log line after the closure consumed it. Both are per-connection,
not per-query, so the blast radius is small.

**Smallest fix.** Pass an `Arc<str>` instead of a `String`, clone the `Arc` for
the closure, and keep the original for logging — one allocation instead of two.
`CertStore::prewarm` takes `&str` (`store.rs:431`), so the call site is
unaffected.

**Cost.** Memory: saves one allocation per DoT connection, at the price of an
`Arc` header on the one that remains. Net roughly neutral in bytes, one fewer
allocator round trip. **Under the 1 MB threshold, so not worth doing on its
own** — fold it into finding 3 if that is ever taken, otherwise leave it.

### 7 — INFORMATIONAL · every allocation oracle is pinned at zero margin

Observed counts and ceilings are in §Measurements. Every ceiling equals the
observed count exactly. The only slack is `JITTER_ALLOWANCE = 4` allocations
spread across 64 iterations — 0.0625 per query.

This is deliberate and it is the right design: any new allocation on these
paths fails the gate immediately. It is also, by the §8 definition, a fragile
pass everywhere, and it should be recorded as one rather than reported as
"passed".

One case is weaker than the rest. `intercept_alloc`'s "intercepted
pass-through GET" measures four batches and only asserts on the last two:

```
(3072, 1309120), (3173, 1728912), (3200, 1835968), (3200, 1835968)
```

It needs three batches to settle. `BATCHES = 4` leaves exactly one batch of
evidence that the value is stable. If the warm-up ever takes one batch longer,
the oracle asserts on two different numbers and fails for a reason that has
nothing to do with a leak. Raising `BATCHES` to 6 costs milliseconds and
removes the ambiguity.

### 8 — INFORMATIONAL · the listener layer and the SNI path have no oracle

Covered by `forward_alloc`: `Pipeline::handle` and below.

Not covered by any oracle:

| Uncovered allocation | Site | Per |
| -------------------- | ---- | --- |
| Datagram copy for the spawned task | `udp.rs:163` | query |
| TCP message buffer | `tcp.rs:204` | query |
| Length-prefix `splice` (F3) | `tcp.rs:223` | query |
| SNI host `Box<str>` | `sni.rs` via `normalize_host` | connection |
| ClientHello buffer growth | `https.rs:401` | connection |
| SNI host `String` for the resolver | `https.rs:300` | connection |

`forward_alloc` calls `Pipeline::handle` directly, so nothing above it is
measured. `proxy_alloc` and `intercept_alloc` enter at the proxy, not at the
TLS accept loop.

`udp_inflight_cost.rs` exists and measures RSS under an upstream outage — a
different question, and it does not bound per-datagram allocation.

Why it was not run here: no oracle exists to run. Named rather than fixed.

### 9 — CLOSED, redesign accepted · `QueryType::Other` allocates per query, against the vocabulary's own reasoning

**Validated and closed, 2026-09-16 — the redesign is accepted.** Both steps are
done: measured, implemented, and confirmed on the RB5009.

**Implementation.** `QueryType::Other(String)` becomes a fixed `Copy` enum —
the ten CONTEXT.md §Record Type labels plus the rest of the `rrtype_bit` set
(SVCB, CAA, DS, DNSKEY, NAPTR) as named variants, and `Other(u16)` carrying the
raw wire code for anything else. `to_fah_query_type` maps each wire type to its
variant with no allocation; `fah_rules::matcher::qtype_bit` and
`fah_stats::bucket::qtype_index` become enum-to-index matches instead of string
compares; the API renders an unknown type as its RFC 3597 `TYPE<n>` spelling.
A drift guard (`every_named_query_type_carries_the_bit_its_rule_spelling_parses_to`)
pins the enum's indices against `rrtype_bit`'s table so the two vocabularies
cannot silently diverge.

**Allocation equality — the authoritative measure, dev box.** `forward_alloc`
gained three `RecordType::HTTPS` cases. Before the redesign HTTPS cost one more
allocation than A on every path; after it:

| cache-hit case | before | after |
| -------------- | -----: | ----: |
| A, inline name | 640 | 640 |
| HTTPS, inline name | 704 | **640** |

HTTPS cache-hit is now bit-for-bit equal to A (miss likewise, 1030 = 1031). The
+1 per query the finding named is gone, not reduced.

**RB5009 / aarch64 functional validation.** The diag-timing probe on veth3
processed A, AAAA, HTTPS (named), SVCB (named → OTHER bucket) and unknown type
65534 (`Other(u16)`) without error or panic; a real `www.example.com HTTPS`
answered `NOERROR`. Every `QueryType` arm runs on the device.

**RB5009 controlled cache-hit experiment.** 50 names pre-warmed for both A and
HTTPS, then six contiguous 5000-query phases against that **frozen** cache, so
the only variable between an A phase and an HTTPS phase is the query type:

| phase | CPU/q | dRSS | dCEST | hits/miss |
| ----- | ----: | ---: | ----: | --------- |
| A #1 | 91.0 µs | −372 KiB | 0 | 5000/0 |
| HTTPS #1 | 75.8 µs | −1384 KiB | 0 | 5000/0 |
| A #2 | 70.8 µs | −40 KiB | 0 | 5000/0 |
| HTTPS #2 | 95.0 µs | −40 KiB | 0 | 5000/0 |
| A #3 | 93.6 µs | −28 KiB | 0 | 5000/0 |
| HTTPS #3 | 93.2 µs | −44 KiB | 0 | 5000/0 |

Every phase pure hits, `dCEST = 0`, cache entries **7907 → 7907** — memory is
not confounded by negative-cache fill. **A mean ≈ 85 µs/q, HTTPS ≈ 88 µs/q, with
fully overlapping ranges (70–95).** No RSS growth attributable to the workload.

**What the device can and cannot show.** The shipped probe has **no counting
allocator** — that is a test-only harness — so the device cannot count
allocations. Allocation equality (640 = 640) is therefore established by the
dev-box oracle; the device establishes runtime equivalence (A and HTTPS
indistinguishable in CPU and memory) and bounded memory (frozen cache, no
growth). A ~1 µs qtype delta would sit below this device measurement's ~85 µs/q
floor regardless, which is exactly why the oracle carries the +0 claim and the
device carries the equivalence claim.

**Verdict:** F9 fully validated and closed; the `QueryType` redesign is
accepted. The corresponding code is in the working tree pending the clean-process
gate and an explicit commit go.

--- the read-only audit's original argument, kept for the record ---

### 9-orig — LOW · `QueryType::Other` allocates per query, against the vocabulary's own reasoning

`crates/fah-dns/src/qtype.rs:19`:

```rust
other => QueryType::Other(other.to_string()),
```

`QueryType` is `A | Aaaa | Other(String)` (`crates/fah-model/src/query.rs:11-15`).
Every query that is not A or AAAA allocates a `String` for its type name, once
per query, in `Pipeline::handle` before the matcher runs.

**Rule broken:** hard rule 3 (no allocations on the hot path) and principle 3.

**CONTEXT.md already argues against this shape.** §Record Type fixes the label
set at eleven — `A`, `AAAA`, `HTTPS`, `MX`, `TXT`, `PTR`, `NS`, `SOA`, `SRV`,
`CNAME`, `OTHER` — and gives the reason: "The set is fixed at compile time
because a per-query counter cannot hold an unbounded set of type strings." The
model carries exactly that unbounded string anyway, one per query.

**What the allocation is spent on.** Nothing. `crates/fah-stats/src/bucket.rs:31-47`
receives it, compares it against nine string literals, converts it to an index
into a fixed array, and drops it. Its own doc comment at `bucket.rs:28-30`
reads "A handful of `==` comparisons for the `Other` case — no allocation, no
regex (hard rule 3)". That is true of `qtype_index` and false of the path: the
allocation was moved one crate upstream, not removed.

**How much traffic.** Not a rare shape. A modern browser navigation sends A,
AAAA **and** HTTPS (type 65) for the same name, so roughly a third of real
client queries take this allocation. PTR, TXT, MX and SVCB add to it.

**No oracle saw it, until step 1 below.** `raw_query` hard-coded
`RecordType::A`, so every DNS figure in §Measurements was measured on the one
query type that does *not* allocate, and all of them are a **lower bound on
real traffic**.

**Step 1 is done, and it confirms the finding.** `forward_alloc` now carries
three `RecordType::HTTPS` cases. Measured at 64 handles, dev box, debug
profile, superseded by re-running
`cargo test -p fah-dns --test forward_alloc -- --nocapture`:

| Case | Observed | Ceiling | Headroom | Per query | vs the A case |
| ---- | -------- | ------- | -------- | --------- | ------------- |
| blocked HTTPS, inline name | 768 | 772 | 4 | 12 | 13 → 12, **−1** |
| cache hit HTTPS, inline name | 704 | 708 | 4 | 11 | 10 → 11, **+1** |
| miss HTTPS, inline name | 1093 | 1156 | 63 | 17.1 | ~16.3 → ~17.1, **+1** |

**+1 per query on the cache-hit and miss paths**, which is the allocation this
finding names. The cache-hit row isolates it cleanly: both types replay a
negative `NOERROR` entry, so the response construction is identical and the
`QueryType::Other` string is the only difference.

**The blocked path is −1, and that is not this finding being wrong.**
`response::blocked` synthesizes no record for a type that is neither A nor
AAAA, so it skips a `Record::from_rdata` and a name clone — two allocations
saved against the one the qtype string costs. Blocking an HTTPS query is
cheaper than blocking an A query, and the qtype string is still there.

**Smallest fix, in two steps.**

1. **DONE.** Add an `HTTPS` case to `forward_alloc` and read the number.
   Test-only, no production change, and it turns this finding into a
   measurement. The numbers are in the table above.

2. If the measurement confirms that this allocation is material, redesign
   `QueryType` around the compile-time-fixed record-type vocabulary already
   specified in CONTEXT.md §Record Type, while preserving an appropriate
   representation for genuinely unknown types. That would remove the per-query
   `String` allocation in `fah-dns` and allow `fah-stats::qtype_index` to use an
   enum-to-index match instead of string comparisons.

**Step 2 is now an owner decision, not a blocked one.** Its condition — "if the
measurement confirms that this allocation is material" — has an answer to judge
against: **+1 out of 11 on a cache hit, roughly 9 %, on about a third of real
queries.** Whether that is material is the call to make; this finding does not
make it. What has not changed is the cost of acting: step 2 touches an L1
crate's public API and a documented JSON field, `qtype` on the query event
(API.md). For the named ten record types the serialization is unchanged; only a
genuinely unknown type's rendering needs care.

**What goes wrong if it is not fixed.** One avoidable allocation on about a
third of real queries, and a measurement set that cannot see it.

**What test would fail if this were already fixed?** None today — step 1 is
what creates it.

### 10 — LOW · the current hot path does not yet satisfy the allocation-free invariant

PERFORMANCE.md states the intended project invariant: the hot path is
allocation-free.

> **No GC, no hidden allocations** — the hot path is allocation-free;
> allocations happen at load/reload time and are served by **mimalloc**.

The current measurements show that the implementation does not fully satisfy
that invariant: there are **10-23 allocations per DNS query** in the current
oracle cases, **50-53 per HTTP request**, plus additional allocations that the
current oracles do not cover — the listener layer (Finding 8) and every
non-A/AAAA query type (Finding 9).

`url_lookup_alloc = 0` confirms that the matcher itself is allocation-free, and
RULE_ENGINE.md §Cost states that narrower claim accurately. But a clean matcher
does not make the overall hot path allocation-free; it makes one stage of it
clean.

**Therefore this is not merely a documentation defect. It is performance debt
against an explicit project invariant.** The invariant was chosen deliberately
and covers the whole relevant hot path, not just the matcher. The gap belongs
to the code.

**Rule broken:** PERFORMANCE.md §Golden rules 4, and hard rule 3.

**The inventory is already in this file.** §Measurements enumerates every
surviving allocation with its site and its frequency — per query, per request,
per connection. That list is the debt register. Nothing there should be read as
permanently settled: a row marked "structural" or "required" carries an
*argument*, not a measurement, and an argument is what gets overturned when
someone measures it.

**How the gap closes.** One row at a time, each either eliminated or given an
explicit measured justification. Findings 4 and 9 are the first two candidates
precisely because they are small and unargued. The larger ones — cloning
`Record`s out of the cache on every hit (`response.rs:129`), the two heap copies
of the domain name (`qtype.rs:27` then `cache.rs:485`), the per-datagram
`to_vec` (`udp.rs:163`) — need a measurement and a design before anyone touches
them, and the measurement comes first.

**What must not happen.** The invariant does not get narrowed to fit the
current implementation. Redefining "hot path" to mean "the matcher" would make
this finding disappear without a single allocation being removed, and would
retire a target that is still worth hitting.

**What goes wrong if it is not addressed.** The number drifts upward. Every
oracle ceiling in §Measurements is pinned exactly at today's count (Finding 7),
so nothing *silently* regresses — but a ceiling raised once per change, each
time for a locally good reason, is how 10 allocations per query becomes 30
without any single commit looking wrong.

**What test would fail if the invariant held?** None asserts it end to end
today. `url_lookup_alloc` asserts it for the matcher with `assert_eq!(…, 0)`.
The equivalent for `Pipeline::handle` is the shape to aim at, and the ceilings
in `forward_alloc` are the ratchet that gets there — each fix lowers one.

**This finding proposes no `.md` edit and makes none.** It records the
discrepancy; the wording is the owner's to approve.

## Measurements

All figures: dev box, x86-64, `rustc 1.96.0`, **debug profile**, single-threaded
tokio runtime, 64 iterations per batch. Superseded by re-running the named
command on the same profile, or by any run on the RB5009 (which these numbers
do not describe).

### Allocation oracles — observed, ceiling, headroom

`cargo test -p fah-dns --test forward_alloc -- --nocapture --test-threads=1`

| Case | Transport | Observed / 64 | Ceiling / 64 | Headroom | Per query |
| ---- | --------- | ------------- | ------------ | -------- | --------- |
| blocked, inline name | all 4 | 832 | 832 + 4 | 4 | 13 |
| blocked, heap name | all 4 | 1216 | 1216 + 4 | 4 | 19 |
| cache hit, inline name | all 4 | 640 | 640 + 4 | 4 | 10 |
| cache hit, heap name | all 4 | 1024 | 1024 + 4 | 4 | 16 |
| miss, inline name | Udp | 1046 | 1088 + 4 | 46 | 16.3 |
| miss, inline name | Tcp | 1030 | 1088 + 4 | 62 | 16.1 |
| miss, inline name | Dot | 1030 | 1088 + 4 | 62 | 16.1 |
| miss, inline name | Doh | 1026 | 1088 + 4 | 66 | 16.0 |
| miss, heap name | Udp | 1477 | 1536 + 4 | 63 | 23.1 |
| miss, heap name | Tcp | 1477 | 1536 + 4 | 63 | 23.1 |
| miss, heap name | Dot | 1472 | 1536 + 4 | 68 | 23.0 |
| miss, heap name | Doh | 1473 | 1536 + 4 | 67 | 23.0 |
| adaptive forward | — | 1216 | first batch + 4 | 4 | 19 |

Miss-path bytes, same run: 164,896 to 243,392 bytes per 64 queries — 2.6 to
3.8 KB per miss. Fresh-hit and blocked batches are bit-identical between the
two batches; miss batches vary by up to 8 allocations, inside the allowance.

`cargo test -p fah-http --test proxy_alloc --test intercept_alloc -- --nocapture --test-threads=1`

| Case | Observed / 64 | Ceiling / 64 | Headroom | Per request |
| ---- | ------------- | ------------ | -------- | ----------- |
| proxy pass-through GET | 3264 | 3264 + 4 | 4 | 51 |
| proxy pass-through, `Connection` header | 3392 | 3392 + 4 | 4 | 53 |
| proxy blocked script | 1280 | 1280 + 4 | 4 | 20 |
| proxy blocked document | 2048 | 2048 + 4 | 4 | 32 |
| intercepted pass-through GET | 3200 | 3200 + 4 | 4 | 50 |
| intercepted blocked script | 1600 | 1600 + 4 | 4 | 25 |
| intercepted blocked document | 2432 | 2432 + 4 | 4 | 38 |

Bytes, same run: proxy pass-through 2,021,696 per 64 = 31.6 KB per request;
blocked script 1,178,176 per 64 = 18.4 KB.

`cargo test -p fah-rules --test url_lookup_alloc --test dedup_alloc_bound`

| Oracle | Observed | Ceiling | Headroom |
| ------ | -------- | ------- | -------- |
| `url_lookup_alloc` | 0 allocations over 6000 lookups | 0, asserted with `assert_eq!` | exact zero — the strongest form available |
| `dedup_alloc_bound` | not printed on success | 40 MiB peak | ~9.5 MiB, from the test's own model of 30.5 MiB at `dedup_alloc_bound.rs:66-69` |

`dedup_alloc_bound` prints nothing on a pass, so its observed peak is the
test's modelled figure, not a reading. Making it print would turn a claim into
a number.

### Allocations — per-query and per-request hits

One row per hit. Once-per-process hits are collapsed at the end of each table.

**Read the Verdict column as debt status, not as absolution.** PERFORMANCE.md's
golden rule 4 makes the hot path allocation-free the project invariant, and
Finding 10 records that the implementation does not meet it yet. So every
per-query, per-request and per-connection row below is outstanding work:

- **"removable"** — no argument for it; eliminate it.
- **"justified" / "required" / "structural"** — an argument why it is hard to
  remove today, made in prose and not backed by a measurement. These are the
  rows to attack with a design and a number, not the rows to stop looking at.
- **"accepted"** — once-per-process, genuinely outside the invariant.

Nothing here is settled except the "accepted" rows. The count is what has to
come down.

#### `fah-dns/src/pipeline.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `pipeline.rs:459` | `key.clone()` | hands the key to the SWR pool | per stale hit with a won claim | **Finding 4** — removable |
| `pipeline.rs:348` | `domain_of(query.name())` → `String` | the matcher and the cache both need a lowercased owned name | per query | justified — folded once for the whole path, see the comment at `344-347` |
| `pipeline.rs:148-159` | 5 × `Arc::new` | constructor | once per process | accepted |
| `pipeline.rs:198` | `Arc::clone`, `forwarder.clone()` | worker spawn | once per process | accepted |

#### `fah-dns/src/udp.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `udp.rs:163` | `buf[..len].to_vec()` | the spawned task needs an owned `'static` datagram | per query | justified by the spawn; removable only by not spawning. **Uncovered by any oracle — Finding 8** |
| `udp.rs:136` | `Arc::new(Listener)` | listener setup | once per process | accepted |

Per query the loop also takes two `Arc::clone`s (`udp.rs:165-166`) — refcount
bumps, no allocation.

#### `fah-dns/src/tcp.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `tcp.rs:204` | `vec![0u8; len]` | read buffer sized from the length prefix | per query | a reusable per-connection buffer would remove it; not proposed without a measurement. **Uncovered — Finding 8** |
| `tcp.rs:223` | `reply.splice(0..0, len)` | RFC 1035 length prefix | per query | **F3, closed — measured.** 13–16 ns at real reply sizes, and the realloc fires only when the reply lands exactly on hickory's capacity. The vectored alternative is *slower* below ~1 KiB. See §TCP length-prefix framing (F3). The earlier proposed fix (encoding at a two-byte offset) is invalid regardless — hickory stores name-compression pointers as absolute buffer indices, so the offset corrupts every one of them |

#### `fah-dns/src/response.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `response.rs:127` | `query.clone()` | echo the question section | per cache hit | required by the wire format |
| `response.rs:129` | `record.clone()` | answer records into the reply | per record, per cache hit | structural — see Decisions |
| `response.rs:134` | `record.clone()` | authority records | per record, per negative cache hit | structural |
| `response.rs:97` | `query.clone()` | blocked reply question | per blocked query | required |
| `response.rs:100`, `106` | `query.name().clone()` | synthesized A / AAAA owner name | per blocked A/AAAA query | required |
| `response.rs:75`, `80` | `query.clone()` | `$dnsrewrite` reply | per rewritten block | required |
| `response.rs:83`, `86` | `query.name().clone()` | rewrite record owner name | per rewritten block | required |
| `response.rs:173` | `message.to_vec()` | wire encoding | per query | unavoidable |
| `response.rs:159` | second `encode` of `message.truncate()` | UDP overflow | per truncated UDP reply | pays twice on truncation only |
| `response.rs:184` | `.to_vec()` on the `SERVFAIL` fallback | encode failure | per encode failure | throttled at `pipeline.rs:164-176` |

#### `fah-dns/src/cache.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `cache.rs:485` | `domain.into()` → `Box<str>` | the key owns its name | per resolve | second copy of a name already owned as a `String`; see Decisions |
| `cache.rs:600` | `response.answers.clone()` | store the positive answer | per cacheable miss | structural |
| `cache.rs:614` | `response.authorities.clone()` | store the negative answer's SOA | per cacheable negative miss | structural (RFC 2308) |
| `cache.rs:630` | `Arc::new(answer)` | so a hit hands back a refcount bump | per insert | justified in the `Entry::answer` doc |
| `cache.rs:640` | `key.clone()` | the FIFO queue node | per insert | needed — the queue outlives nothing, but it does need its own key |
| `cache.rs:454` | `.collect()` | shard vector | once per process | accepted |
| `cache.rs:337` | `keys().next().cloned()` | broken-invariant fallback in `evict_oldest` | never, by invariant | accepted |

#### `fah-dns/src/dot.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `dot.rs:136` | `host.to_owned()` | `'static` for `spawn_blocking` | per connection | **Finding 6** |
| `dot.rs:173` | `host.clone()` | so the host can be logged after the move | per connection | **Finding 6** — removable |
| `dot.rs:98` | `tls.clone()` | two `Arc` bumps | per connection | no allocation |
| `dot.rs:39-68` | 4 × `Arc::new` | TLS config and the semaphore | once per process | accepted |

#### `fah-dns/src/swr.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `swr.rs:203` | `key.clone()` in `refresh` | `store` takes the key by value | per refresh | background pool, off the query path |
| `swr.rs:229-234` | `refresh_query` builds a `Message` | the refresh is a new query, not a replay | per refresh | justified in the doc comment at `216-227` |
| `swr.rs:96`, `160` | `Mutex::new`, `Arc::new` | pool setup | once per process | accepted |

#### `fah-dns/src/qtype.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `qtype.rs:19` | `other.to_string()` | `QueryType::Other` owns a `String` | per non-A/AAAA query | **Finding 9** — and unmeasured |
| `qtype.rs:27-28` | `String::with_capacity` + `write!` | wire-format name as text | per query | sized once; the test at `qtype.rs:73-83` asserts exactly one sizing for ASCII names |

#### `fah-dns/src/rewrite.rs`

N/A — zero hits in every category. `interpret` parses in place and returns a
`Copy` enum. Verified by grep over lines 1-113.

#### `fah-http/src/proxy.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `proxy.rs:579` | `request_host(...).to_string()` | the event owns its host | per request | captured before the head is forwarded — see the `judge` doc |
| `proxy.rs:584` | `path_and_query(...).to_string()` | the event owns its path | per request | same |
| `proxy.rs:613` | `method().as_str().to_string()` | the event owns its method | per request | could be a `&'static str` for the nine standard methods; `fah-model` change, not proposed |
| `proxy.rs:581` | `absolute_url(...)` | the matcher needs a full URL | per request | `request.rs:191` sizes it in one `with_capacity` — already the cheap shape |
| `proxy.rs:484` | `claim.host.clone()` | `HostResolver::resolve` takes `String` | per request with a name host | removable only by changing the `HostResolver` trait |
| `proxy.rs:483` | `vec![ip]` | one-element vector for the literal case | per request with an IP-literal host | avoidable with a `SmallVec`-shaped return; not worth a dependency |
| `proxy.rs:692` | `.collect::<Vec<HeaderName>>()` | `Connection`-named headers must be read before they are removed | per request and per response | the comment at `691` states the reason; the `Vec` is empty for the common case, and an empty `Vec` does not allocate |
| `proxy.rs:202` | `format!` | upstream connector error text | per failed connect | error path |
| `proxy.rs:278-280` | 2 × `Arc::new` | constructor | once per process | accepted |
| `proxy.rs:186` | `Box<dyn Future>` | the connector's associated type | per upstream connect | hyper's trait shape, not ours |

#### `fah-http/src/https.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `https.rs:130` | `Vec::with_capacity(HELLO_CHUNK)` | ClientHello buffer | per connection | sized once for the common case |
| `https.rs:401` | `hello.reserve(...)` | buffer growth | per read past the first chunk | bounded by `MAX_HELLO_BYTES` |
| `https.rs:300` | `host.to_string()` | `HostResolver::resolve` takes `String` | per connection | same trait constraint as `proxy.rs:484` |
| `https.rs:379` | `host.to_string()` | the event owns its host | per connection | required |
| `https.rs:63-65` | 2 × `Arc::new` | constructor | once per process | accepted |

#### `fah-http/src/sni.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `sni.rs:147` | `String::from_utf8_lossy` | names a rejected SNI in the log | per rejected SNI, **only when `debug` is enabled** | see §Error and observability pressure |

The scan itself allocates nothing: `scan_client_hello` walks into a
`[u8; MAX_NAME_LEN]` stack buffer (`sni.rs:136`) and only `normalize_host`
produces the `Box<str>`.

#### `fah-http/src/claim.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `claim.rs:90` | `host.to_string()` | `Destination` owns its host | per request | required — the borrow is from a header the forward consumes |
| `claim.rs:105` | `text.parse::<Authority>()` | validate the `Host` header | per request | `Authority` copies into `Bytes` |
| `claim.rs:118` | `uri().authority().cloned()` | absolute-form request target | per request | `Bytes` refcount bump, no allocation |
| `claim.rs:156` | `String::with_capacity(SOCKET_ADDR_TEXT_MAX)` | render an address | per call | sized once |

#### `fah-http/src/block.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `block.rs:113` | `String::with_capacity(512)` | the 403 page | per blocked document | sized once; this is the bulk of the 32-allocation blocked-document figure |
| `block.rs:126` | `escape_into` | HTML-escape the rule text | per blocked document with a rule | writes into the existing buffer, no second allocation |
| remaining 9 hits | `HeaderValue::from_static`, `Bytes::from_static` | headers and the tiny bodies | per blocked request | static, no allocation |

#### `fah-http/src/request.rs`

| Site | Construct | Why | Frequency | Verdict |
| ---- | --------- | --- | --------- | ------- |
| `request.rs:191-198` | `String::with_capacity` + 5 × `push_str` | build the absolute URL | per request | one allocation, sized exactly — already the cheap shape |

#### `fah-http/src/domain.rs`, `tls_server.rs`, `server.rs`, `intercept.rs`

| File | Per-connection allocations | Verdict |
| ---- | -------------------------- | ------- |
| `domain.rs` | none — `Accepted` moves its fields, never clones | clean |
| `tls_server.rs` | none on the accept path; 2 × `Arc::new` at `36-37` once per process | clean |
| `server.rs` | 2 `Arc::clone` per connection (`server.rs:263`, `272`), refcount only; `Arc::new` ×2 at `89` once per process | clean |
| `intercept.rs` | `frame_for` clones a `PathAndQuery` (`intercept.rs:537`) and a `HeaderValue` (`551`) per intercepted request; both are `Bytes`-backed refcount bumps | clean |

### TCP length-prefix framing (F3)

Dev box, x86-64, `rustc 1.96.0`, **release profile**, mimalloc, criterion 0.5,
7 reply sizes × 2 capacity shapes, 2026-09-16. Superseded by re-running
`cargo bench -p fah-dns --bench frame_reply`.

Two realistic implementations, not one against an empty operation:

- **splice** — `frame_reply`'s construct verbatim, front-insertion into the
  reply `Vec`.
- **iovec** — the prefix plus the `[IoSlice; 64]` array tokio itself builds
  per write (`tokio-1.53.1/src/io/util/write_all_buf.rs:50`), which is the
  work a vectored write would actually add.

Capacity shapes model hickory's encoder, which starts at
`Vec::with_capacity(512)` and doubles (`hickory-proto-0.26.1/src/op/message.rs:503`):
**spare** = the reply fits inside that capacity; **exact** = the reply ends
exactly on it, so the prepend must grow.

Time per framing operation, median of 100 samples:

| Reply | splice, spare | splice, exact | iovec |
| ----: | ------------: | ------------: | ----: |
| 64 B | 13.4 ns | 26.3 ns | 16.3 ns |
| 256 B | 15.6 ns | 33.1 ns | 16.5 ns |
| 500 B | 14.5 ns | 50.9 ns | 16.4 ns |
| 512 B | 29.1 ns | 48.2 ns | 16.7 ns |
| 1 KiB | 53.6 ns | 91.7 ns | 17.0 ns |
| 4 KiB | 60.3 ns | 141.4 ns | 16.2 ns |
| 16 KiB | 209.7 ns | 413.1 ns | 17.1 ns |

Allocations, one framing operation per row, counting allocator over mimalloc:

| Reply | Shape | Capacity before | Capacity after | Allocations | Bytes |
| ----: | ----- | --------------: | -------------: | ----------: | ----: |
| 500 B | spare | 512 | 512 | 0 | 0 |
| 500 B | exact | 500 | 1000 | 1 | 1000 |
| 512 B | spare | 1024 | 1024 | 0 | 0 |
| 512 B | exact | 512 | 1024 | 1 | 1024 |
| 16 KiB | spare | 32768 | 32768 | 0 | 0 |
| 16 KiB | exact | 16384 | 32768 | 1 | 32768 |

`iovec` allocates zero at every size and shape. Sizes 64 B, 256 B, 1 KiB and
4 KiB follow the same pattern and are omitted.

What the numbers settle:

- **The 1 µs screening gate is not reached at any size**, worst case included
  (16 KiB, exact capacity, 413 ns). At the sizes real traffic produces the cost
  is 13–16 ns, under 0.2 % of a query.
- **The realloc is not a per-reply cost.** It fires only in the exact shape,
  which needs the encoded message to land precisely on a power-of-two boundary.
- **The vectored alternative is slower below ~1 KiB.** Its ~16.4 ns is flat
  across every size because the `[IoSlice; 64]` array — 1 KiB of stack — is
  built per write regardless of payload. Under 512 B the memmove costs less
  than that array. The crossover sits between 512 B and 1 KiB.
- The iovec figure is **optimistic**: it measures slice construction only, not
  the `writev` syscall, which costs marginally more kernel-side than a plain
  `write`. On DoT the gap is smaller still, since rustls copies the plaintext
  into its own record buffer either way.

Two limits on the claim. The splice spare-capacity column reads as a slope, not
as point values — the 512 B row (29.1 ns) sits off-trend against 500 B
(14.5 ns) because that shape doubles the touched buffer, which is criterion
batch cache pressure rather than framing cost. And DNS reaches TCP precisely
when the reply is large, so the size distribution on this path is heavier than
UDP's; at the 16 KiB ceiling `MAX_MESSAGE_LEN` allows, the vectored path would
save ~190 ns on this box, ~1.7 µs on the RB5009 at the ~9× factor, against the
~85 µs per query the device measures.

### Locks

| Site | Lock | Frequency | Verdict |
| ---- | ---- | --------- | ------- |
| `cache.rs:519` | `Mutex<Shard>` in `lookup_inner` | per resolve | **F2 — already argued** in the file header and ARCHITECTURE.md §Runtime Model. Cited, not re-reported. The `.unwrap()` on the guard is Finding 1 |
| `cache.rs:626` | same, in `insert` | per cacheable miss | F2 |
| `cache.rs:555` | same, in `suppress_refresh` | per failed refresh | F2, background |
| `cache.rs:566` | same, in `release_refresh_claim` | per dropped SWR job | F2, background |
| `cache.rs:719` | same, in `stats` | per admin request | off the hot path |
| `cache.rs:778` | same, in `clean` | per sweep, on `spawn_blocking` | off the hot path by construction (`pipeline.rs:226-236`); one shard at a time, so a concurrent resolve waits at most one shard's walk |
| `swr.rs:155` | `Mutex<Option<Receiver>>` | once, at worker spawn | accepted |
| `swr.rs:173` | `tokio::sync::Mutex<Receiver>` held across `recv().await` | per refresh job | **serializes only the `recv`** — the scope at `171-178` releases it before the forward, so N workers still refresh concurrently. Intended, and the comment says so |
| `intercept.rs:486` | `tokio::sync::Mutex<Option<Sender>>` | per intercepted request | **serializes per origin session.** Held across `connect_verified_upstream` and `handshake` on a cold session, which is correct — it collapses N concurrent first requests into one TLS handshake. Dropped before the H2 send (`intercept.rs:502`); held across the H1 round trip, which HTTP/1.1 requires |
| `tcp.rs:98`, `dot.rs:68`, `server.rs:89`, `tls_server.rs:36` | `Semaphore` | per connection | the connection ceiling; `acquire_owned` before `accept` is deliberate and correct (see Decisions) |
| `fah-common` `LogThrottle` | none | per throttled log | verified lock-free: `throttle.rs:22-40` is `fetch_add` plus a `compare_exchange_weak` loop |

`pipeline.rs`, `udp.rs`, `response.rs`, `qtype.rs`, `rewrite.rs`, `https.rs`,
`sni.rs`, `proxy.rs`, `claim.rs`, `request.rs`, `block.rs`, `domain.rs`:
**N/A — zero lock constructs in production code.** Counted with the tuned
pattern, which excludes `.read(` and `.write(` because in these crates those
are socket calls.

### Clocks, IO and thread dispatch

| Site | Construct | Frequency | Verdict |
| ---- | --------- | --------- | ------- |
| `pipeline.rs:353` | `Instant::now()` | per query | latency measurement; vDSO read, no syscall |
| `pipeline.rs:434` | `SystemTime::now()` | per query | the event's wall-clock timestamp |
| `cache.rs:523` | `Instant::now()` under the shard lock | per resolve | inside the critical section; moving it out would mean reading the clock even on a miss |
| `cache.rs:627` | `Instant::now()` under the shard lock | per insert | same |
| `pipeline.rs:166` | `Instant::now()` for the throttle | per encode failure | error path only |
| `udp.rs:178` | `Instant::now()` for the throttle | per send failure | error path only |
| `https.rs:128`, `240` | `Instant::now()`, `elapsed()` | per connection | required for the event |
| `https.rs:414-424` | `idle_watchdog` sleep loop | per splice | one timer, re-armed against the last activity stamp |
| `proxy.rs:369` | `Instant::now()` | per request | required for the event |
| `dot.rs:180` | `spawn_blocking` | per DoT connection | **Finding 3** |
| `pipeline.rs:243` | `spawn_blocking` | per sweep | correct — `clean` is O(entries) and belongs off the query path |
| `response.rs`, `rewrite.rs`, `sni.rs`, `request.rs`, `claim.rs`, `block.rs`, `tls_server.rs`, `domain.rs` | — | — | N/A, no clock or IO construct in production code |

No `fs::`, no `File::`, no `env::var`, no `std::thread`, no `rand::` on any
path in scope.

### Regex and formatting

| Site | Construct | Frequency | Verdict |
| ---- | --------- | --------- | ------- |
| `qtype.rs:28` | `write!` into a pre-sized `String` | per query | the only formatting on the DNS query path; `Display` walk over the name's labels |
| `proxy.rs:202` | `format!` | per failed upstream connect | error path |
| `block.rs:113-140` | `push_str` chain | per blocked document | no `format!`, deliberately |

**No `Regex` anywhere in either crate.** Verified with the tuned pattern over
all 19 files. Hard rule 3 holds on this axis.

## Error and observability pressure

One row per error path on a hot-path file. "Per occurrence" is what a single
repeated failure costs.

**A note that changes how these read.** `tracing`'s event macro puts every
field expression inside `if enabled` — `macros.rs:627-645` of `tracing 0.1.44`:

```rust
let enabled = $crate::level_enabled!($lvl) && { ... };
if enabled { (|value_set| { ... })($crate::valueset_all!(...)); }
```

The `else` branch at `647` hands the same expression to `__tracing_log!`, which
under the `log` feature is guarded by `if level <= log::max_level()`
(`macros.rs:3271`). This workspace installs no `log` logger — no `LogTracer`,
no `log::set_boxed_logger` anywhere in `crates/` — so `log::max_level()` is
`Off` and that branch is never taken. **At the default `level = "info"`
(`fah-config/src/schema/log.rs:24-26`), a `debug!` or `trace!` site costs one
level compare and evaluates none of its fields.**

| Path | Site | Per occurrence at `info` | Per occurrence at `debug` | Rate limit |
| ---- | ---- | ------------------------ | ------------------------- | ---------- |
| Malformed DNS packet | `pipeline.rs:331` | level compare | — (`trace`) | level only. Acceptable: the packet is dropped with no reply, so there is no amplification |
| DNS encode failure | `pipeline.rs:166-176` | throttled `warn!` with a running count, else nothing | same | **`LogThrottle`, 60 s** |
| UDP reply send failure | `udp.rs:177-182` | throttled `warn!` with a count, else nothing | one line | **`LogThrottle`, 60 s.** This is **F1, and it looks closed** |
| UDP client unreachable | `udp.rs:174` | level compare | one line | level only; the kind check at `186-192` keeps it out of `warn` |
| UDP recv failure | `udp.rs:141-147` | `warn!` + backoff sleep | same | **`RetryPolicy` backoff**, and fatal after the policy gives up |
| TCP oversize message | `tcp.rs:184-190` | counter increment + level compare | one line + close | counter is unthrottled but free; the connection closes, so one per connection |
| TCP/DoT connection error | `tcp.rs:146-152` | throttled `warn!`, else nothing | one line | **`LogThrottle`, 60 s** |
| DoT leaf pre-warm failure | `tcp.rs:157-165` | throttled `warn!`, else nothing | one line | **`LogThrottle`, 60 s** |
| DoT ClientHello rejected / timed out | `dot.rs:127`, `131`, `144`, `148` | level compare | one line per connection | level only. Remote-drivable, but only at `debug` |
| DoT accept failure | `dot.rs:80-86` | `warn!` + backoff | same | **`RetryPolicy`** |
| HTTPS no ClientHello / deadline | `https.rs:139`, `146` | level compare + counter | one line per connection | counter, then level |
| HTTPS non-TLS bytes | `https.rs:153` | level compare + counter | one line per connection | counter, then level |
| HTTPS IP-literal SNI refused | `https.rs:167` | level compare + counter | one line per connection | counter, then level |
| HTTPS blocked at SNI | `https.rs:172` | level compare + counter + one event | one line | the event channel sheds rather than blocks |
| SNI name rejected | `sni.rs:144-149` | level compare, **`from_utf8_lossy` not evaluated** | one line **plus one allocation** | level only |
| HTTP non-HTTP bytes | `proxy.rs:357-359` | level compare + counter | one line per connection | counter, then level |
| HTTP unusable `Host` | `proxy.rs:376` | level compare + counter | one line per request | counter, then level |
| HTTP upstream failure | `proxy.rs:434` | level compare + counter | one line per request | counter, then level |
| HTTP resolve failure | `proxy.rs:491` | level compare + counter | one line per request | counter, then level |
| Accept failure, HTTP/HTTPS | `server.rs:236-247` | throttled `warn!` + backoff sleep | same | **`LogThrottle` + `RetryPolicy`**, and the warning names the sustained count |
| Domain removed from rotation | `server.rs:320-324` | **`error!`, always emitted** | same | none — but it fires at most once per domain, so it is bounded by the domain count |
| No domain left | `server.rs:330` | **`error!`, always emitted, one per accepted connection** | same | **nothing — Finding 5** |
| Cache sweep panicked | `pipeline.rs:245-246` | `warn!`, then `continue` | same | once per interval — but see **Finding 1**, the shard it panicked in is now poisoned |
| Event channel full | `pipeline.rs:450-452`, `proxy.rs:648-651` | counter increment, no log | same | **counter only, by design** — ARCHITECTURE.md §Runtime Model: a slow consumer drops events rather than back-pressuring the pipeline |
| SWR queue full | `swr.rs:124-127` | counter + claim released | same | counter only; the claim goes back so the next stale hit can retry |

Every path an unauthenticated remote can drive is either throttled,
counter-only, or gated behind a level that is off by default. **The one
exception is `server.rs:330`** — Finding 5.

## Memory

### New state

**N/A — this is not a DIFF-mode audit.** No new struct field or static was
introduced; nothing was changed. The existing state is covered under
"accumulation and retained" below.

### Lifetimes

Every spawned task, socket, timer, buffer, permit and gauge guard in scope.
A missing column is the finding.

| What | Who drops it | Happy | Error | Timeout | Abort |
| ---- | ------------ | ----- | ----- | ------- | ----- |
| `Admitted<S>` (UDP in-flight) | `Drop` at `udp.rs:92-96` | yes | yes | n/a | yes — `Drop` runs when the task is dropped |
| UDP datagram `Vec` | task scope | yes | yes | n/a | yes |
| UDP 64 KiB recv buffer | `run`'s stack frame | listener lifetime | listener lifetime | n/a | on listener abort |
| `OpenConnection` (TCP DNS) | explicit `drop` at `tcp.rs:130`, plus `Drop` | yes | yes | yes | yes — `Drop` covers the explicit one being skipped |
| `OwnedSemaphorePermit` (TCP DNS) | explicit `drop` at `tcp.rs:131`, plus `Drop` | yes | yes | yes | yes |
| TCP per-message `Vec` | loop scope, `tcp.rs:204` | yes | yes | yes | yes |
| TCP idle timer | `timeout` future, `tcp.rs:180`, `206` | yes | yes | yes | yes |
| `OpenConnection` (DoT) | explicit `drop` at `dot.rs:104`, plus `Drop` | yes | yes | yes | yes |
| DoT semaphore slot | `let _slot = slot` at `dot.rs:100` | yes | yes | yes | yes |
| DoT handshake deadline | `timeout_at`, `dot.rs:116`, `140` | yes | yes | yes | yes |
| DoT `spawn_blocking` join | `dot.rs:180` | yes | yes | n/a | **the join is dropped; the blocking closure keeps running — Finding 3** |
| TLS stream shutdown | `timeout(TCP_IDLE_TIMEOUT, …)` at `dot.rs:161` | yes | yes | yes | skipped on abort — the socket still closes on drop |
| SWR queue claim | `offer` releases on full/closed (`swr.rs:120-128`); `refresh` clears it by replacing the entry, or sets a cooldown on failure (`swr.rs:207-211`) | yes | yes | **the lease expires on its own** — `Entry::refresh_suppressed_until` is a deadline, not a flag, so a worker that panicked or was aborted cannot strand the entry | yes, same |
| SWR worker handles | returned to the binary by `spawn_workers` | binary aborts them | — | — | yes |
| Cache sweep task | `spawn_cache_cleanup` returns the handle | binary aborts it | `continue` on `JoinError` | — | yes |
| `Accepted` (permit + `OpenConnection`, HTTP/HTTPS) | `Accepted::serve`'s `async move` block, `domain.rs:40-45` | yes | yes | yes | yes — both are moved into the future, so dropping it drops them |
| `Accepted` across the domain handoff | `register` / `detach` move both guards into the new value; both log and return `None` on failure, dropping the guards (`domain.rs:63-66`, `84-87`) | yes | yes | n/a | yes |
| ClientHello buffer | moved into `splice`, explicitly dropped at `https.rs:233` after the write | yes | yes | yes | yes |
| Splice byte counters and `Activity` wrappers | `splice`'s stack frame | yes | yes | yes | yes |
| `idle_watchdog` timer | `select!` arm, `https.rs:254` | dropped when `copy` wins | yes | it is the timeout | yes |
| hyper connection future | `serve_connection`, `proxy.rs:349` | yes | yes | `header_read_timeout` | yes |
| Intercepted upstream `Sender` | `Mutex<Option<Sender>>`, replaced on a closed session (`intercept.rs:488-497`) | yes | yes | yes | yes |

No row is missing a path. `Admitted`, `OpenConnection`, the semaphore permits
and the SWR claim are all present, as required.

### Cancellation

Every `.await` on a path that can be dropped.

| Await | Dropped by | What is left half-done |
| ----- | ---------- | ---------------------- |
| `dot.rs:180` `spawn_blocking(…).await` | task abort at shutdown | **the blocking closure runs to completion, detached.** It holds an `Arc<CertStore>` and mints one leaf. Bounded at one per connection, but it is real. **Finding 3** |
| `tcp.rs:180`, `206` `timeout(read_exact)` | the timeout itself | **a half-read message.** `read_exact` is not cancel-safe, so bytes already consumed are lost. Correct here: both sites `return Ok(())` and close the connection rather than try to resume |
| `pipeline.rs:389` `forwarder.forward(…).await` | connection drop, task abort | the upstream query is abandoned. Nothing is written to the cache, no claim was taken on this path, so nothing is stranded |
| `swr.rs:173` `rx.lock().await` then `recv().await` | worker abort | `recv` is cancel-safe and the `tokio` guard releases on drop. No message is lost |
| `swr.rs:200` `forward(…).await` inside `refresh` | worker abort | **the refresh claim is not released.** Correct by design: `refresh_suppressed_until` is a deadline, so the claim self-expires after `refresh_claim_lease`. The `Entry` doc at `cache.rs:144-152` argues exactly this |
| `pipeline.rs:243` `spawn_blocking(clean).await` | sweep task abort | the sweep runs to completion detached, holding `Arc<DnsCache>`. Harmless — and see Finding 1 for what happens if it panics instead |
| `https.rs:131` `timeout(read_client_hello)` | the timeout | the partial `hello` buffer is dropped with the connection. Nothing forwarded |
| `https.rs:253` `select!` on `copy` vs `idle_watchdog` | either arm | **an in-flight `copy_bidirectional` chunk.** Whichever direction was mid-write is abandoned and both sockets close. Correct for an idle-timeout close |
| `https.rs:195` `timeout(TcpStream::connect)` | the timeout | a half-open connect; the socket drops |
| `intercept.rs:486` `sender.lock().await` | request cancel | the guard releases on drop. If cancelled during `connect_verified_upstream`, no session is stored and the next request retries |
| `proxy.rs:421` `client.request(upstream).await` | client disconnect | the upstream request is abandoned mid-flight; hyper closes or returns the connection to its pool |
| `server.rs:314` `Rotation::send(…).await` | acceptor abort | **the `Handoff` is dropped, taking the permit and `OpenConnection` with it.** Correct — both are RAII |

Nothing here is a defect. Two rows are worth remembering: the detached
`spawn_blocking` (Finding 3) and the self-expiring SWR claim, which is the one
piece of state that survives its owner and is designed to.

### Accumulation and retained

Everything keyed by client, host, connection or query, plus every collection in
scope.

| What | Keyed by | Lifetime | What caps the key space |
| ---- | -------- | -------- | ----------------------- |
| `DnsCache::shards` | — | process | `SHARD_COUNT`, a compile-time constant |
| `Shard::map` | domain + qtype + qclass | process, evicted | **`capacity` per shard**, from `[dns.cache] max_entries`; `over_bounds()` at `cache.rs:359` also enforces `byte_capacity`. `insert` evicts until both hold (`cache.rs:652-656`) |
| `Shard::queue` | same key, plus a `seq` | process, compacted | **`capacity * 2`** — `compact()` at `cache.rs:365-370` sweeps ghosts past that threshold. `clean` sweeps ungated so an idle cache's ghosts are not pinned (`cache.rs:383-386`) |
| `Shard::bytes` | — | process | a running total, `saturating_sub` on both removal paths so it cannot wrap to `u64::MAX` and pin `over_bounds()` true — argued at `cache.rs:642-648` |
| `CachedAnswer.records` / `.authorities` | per entry | until eviction | the entry's own byte cost is counted by `entry_heap_bytes`, so a large answer evicts more neighbours rather than escaping the bound |
| SWR queue | cache key | until a worker takes it | **`workers * QUEUE_DEPTH_PER_WORKER`**, a bounded `mpsc`; `try_send` drops and releases the claim when full |
| `UdpInflightGauge` | — | process | fixed-size atomics; `limit` caps concurrent datagrams |
| `TcpConnectionGauge` / `ConnectionGauge` | — | process | fixed-size atomics |
| `LogThrottle` | — | process | two atomics, 24 bytes |
| UDP recv buffer | — | listener | fixed `[u8; 65535]` on the accept loop's stack, reused every iteration |
| UDP per-datagram `Vec` | — | one task | `len`, at most 65535 |
| TCP per-message `Vec` | — | one loop iteration | **`MAX_MESSAGE_LEN = 16 KiB`**, checked at `tcp.rs:183` before the allocation |
| ClientHello `Vec` | — | one connection | **`MAX_HELLO_BYTES = 16 KiB`**, checked at `https.rs:396` |
| `Rotation::senders` | — | process | one per domain thread; only ever shrinks |
| Event channel | — | process | bounded `mpsc`; `try_send` sheds and counts |
| Connection semaphores | — | process | `max_connections`, `DOT_MAX_CONNECTIONS = 64` |
| Intercepted session map | origin host | — | **not in this audit's scope** — `intercept.rs`'s session store is reached two levels deep from `serve_connection`. Named so it is not mistaken for covered |

**Nothing in scope grows with traffic or uptime.** Hard rule 4 holds on every
row above.

**Memory delta:** zero. Nothing was changed. The fixes proposed in findings 4
and 6 each save one allocation per event on their paths — well under the 1 MB
threshold, which is why finding 6 says not to do it on its own.

## Rust quality

### Panics

| Site | Construct | Can it fire? | What happens |
| ---- | --------- | ------------ | ------------ |
| `cache.rs:519`, `555`, `566`, `626`, `719`, `778` | `.lock().unwrap()` | **yes, after any panic under the shard lock** | **Finding 1** — permanent, silent, per-shard |
| `cache.rs:99` | `debug_assert!(false, …)` in `CacheKey::name` | debug builds only | the release path falls back to `Name::root()`, which refreshes nothing and is counted as a failed refresh. The doc at `89-96` argues this, and it is right: panicking on a background task would be worse |
| `cache.rs:335` | `debug_assert!` in `evict_oldest` | debug builds only | release falls through to dropping an arbitrary entry — stays bounded |
| `cache.rs:480` | `debug_assert!` on the lowercase precondition | debug builds only | catches a caller that forgot to case-fold; release trusts `pipeline.rs:349` |

**No `.unwrap()`, `.expect()`, `panic!`, `unreachable!`, `todo!` or
`assert!` on any production path in** `pipeline.rs`, `udp.rs`, `tcp.rs`,
`dot.rs`, `response.rs`, `qtype.rs`, `rewrite.rs`, `swr.rs` (its one hit is
`cache.rs`-style and listed above), `proxy.rs`, `https.rs`, `sni.rs`,
`tls_server.rs`, `server.rs`, `domain.rs`, `claim.rs`, `request.rs`,
`block.rs`, `intercept.rs`. Counted with the tuned pattern, test tails cut.

### Fallbacks that hide something

| Site | Construct | What the fallback hides if it is wrong |
| ---- | --------- | -------------------------------------- |
| `tcp.rs:222` | `u16::try_from(reply.len()).unwrap_or(u16::MAX)` | **a reply over 65535 bytes would be framed as 65535**, so the client reads a short message and every following message on that connection is misaligned. **Currently unreachable**: TCP callers pass `budget = u16::MAX`, and `encode_for_transport` at `response.rs:156-159` truncates whenever `encoded.bytes.len() > budget as usize`. So `reply.len() <= 65535` always holds. **The invariant rests on that one comparison, not on a test.** Worth a test that feeds a >64 KiB message through the TCP path and asserts the `TC` bit rather than a mis-framed prefix |
| `cache.rs:97-102` | `Name::from_ascii(…).unwrap_or_else(…)` | a key holding an unparseable name refreshes the root instead. Counted as a failed refresh, so it shows up in `SwrStats::failed` rather than vanishing |
| `udp.rs:83` | `u64::try_from(count).unwrap_or(u64::MAX)` | a `usize` over `u64::MAX` — impossible on 64-bit |
| `https.rs:427` | `as_millis(…).min(u128::from(u64::MAX))` | a duration over 584 million years |
| `proxy.rs:583` | `path_and_query().map_or("/", …)` | a request with no path logs `/`. Correct for origin-form |

### Slice indexing, arithmetic and casts — checked by hand

| Site | Concern | Verdict |
| ---- | ------- | ------- |
| `udp.rs:163` | `buf[..len]` | `len` comes from `recv_from` into a `[u8; 65535]`, so `len <= 65535`. Safe |
| `sni.rs:50` | `self.buf[start + 3]`, `[start + 4]` | guarded by `start + RECORD_HEADER > self.buf.len()` at `42`. Safe |
| `sni.rs:74`, `115` | `self.buf[self.at]`, `self.buf[self.at..self.at + step]` | `fill()` guarantees `at < record_end <= buf.len()`; `step` is `min(record_end - at, …)`. Safe |
| `sni.rs:115` | `out[written..written + step]` | `step` is `min(…, out.len() - written)`. Safe |
| `sni.rs:137`, `147` | `name[..len]` | `len` is only ever set where `name_len <= MAX_NAME_LEN` is checked (`sni.rs:250`). Safe |
| `sni.rs:195`, `246` | `consumed + 2 + extensions_len`, `list_len + 2` | `usize` adds of values bounded by `2^24` and `2^16`. No overflow |
| `cache.rs:522` | `(entry.expires_at() - now).as_secs() as u32` | truncates past 136 years; `ttl` is capped by `[dns.cache] max_ttl`. Safe |
| `cache.rs:820` | `duration.as_micros() as u64` | truncates past ~584,000 years. Safe |
| `cache.rs:366` | `self.capacity * 2` | `capacity` derives from `max_entries / SHARD_COUNT`. No realistic overflow |
| `tcp.rs:182` | `u16::from_be_bytes(len_buf) as usize` | widening. Safe |
| `swr.rs:88` | `workers * QUEUE_DEPTH_PER_WORKER` | `workers` is a `u32` from config, widened to `usize`. Safe on 64-bit |
| `https.rs:396` | `MAX_HELLO_BYTES - hello.len()` | **would underflow if `hello.len()` exceeded the max.** It cannot: `read_buf` is given `hello.limit(want)`, so the buffer can never grow past `MAX_HELLO_BYTES`, and `want == 0` returns at `398`. Safe, but the safety is two lines apart from the subtraction |

### Guards across `.await`

Recipe applied: every `async fn` in scope was listed, then every `std::sync`
guard inside one was checked against the `.await`s before its drop.

**Result: none.** Every `std::sync::MutexGuard` in scope lives in a synchronous
function — `lookup_inner`, `insert`, `store`, `clean`, `stats`,
`suppress_refresh`, `release_refresh_claim`, and the temporary at `swr.rs:155`.
`clean` is synchronous and reaches an async context only through
`spawn_blocking`, which is the correct shape.

Two `tokio::sync::Mutex` guards are held across `.await` and both are
deliberate — `swr.rs:173` and `intercept.rs:486`. What each serializes, and
whether that is intended, is in the Locks table above. Both are.

### `unsafe`

**None.** `grep -rn "unsafe" crates/fah-dns/src crates/fah-http/src` returns
nothing. No `// SAFETY:` comment is owed.

### `Send` / `Sync`

**N/A — not a DIFF-mode audit.** No field was added or changed, so no
containing type's `Send` or `Sync` status moved.

## Files changed

The audit itself is read-only: no production file was changed by it, and this
document was written at the owner's explicit instruction naming this path.

Two benchmark-only files were added afterwards, to settle F3:

| File | What | Production impact |
| ---- | ---- | ----------------- |
| `crates/fah-dns/benches/frame_reply.rs` | new — the F3 screening bench behind §TCP length-prefix framing | none; nothing in `crates/*/src` references it |
| `crates/fah-dns/Cargo.toml` | one `[[bench]]` entry registering it with `harness = false` | none; build metadata only |

`tcp.rs` is unchanged. The bench is kept rather than deleted because it is
cheap, reproducible, and is the reason no complexity was added to the framing
path — the argument for *not* changing `tcp.rs` needs its evidence to stay
runnable.

## Remaining TODOs

Ordered by what buys the most for the least. **Status is the only part of this
document that moves**; the findings above are the record as taken and are not
rewritten when one is fixed. Read status here, evidence there.

| # | Action | Status | Why now |
| - | ------ | ------ | -------- |
| 1 | Apply Finding 1 — six `.unwrap_or_else(PoisonError::into_inner)` | **FIXED** `3ce7ec5` | Two-line change, no cost on any axis, removes a silent permanent failure mode the supervision tick cannot see |
| 2 | Measure Finding 2 on the RB5009, at `http_runtimes = 2` | **CLOSED — measured, no code change** | Up to 32 drippers (16× the domains) moved p95 not at all; the 16 KiB cap + hello_timeout + max_connections neutralise it. See Finding 2 |
| 3 | Apply Finding 4 — drop the `key.clone()` at `pipeline.rs:459` | **FIXED** `c93e2d1` | One character shorter, one allocation fewer |
| 4 | Apply Finding 5 — move the "no domain left" `error!` to the transition | **FIXED** `bb15de7` | Small, and it protects the log buffer that would explain the failure |
| 5 | Finding 9: measure, then redesign `QueryType` | **CLOSED — redesign accepted, validated** | `forward_alloc` HTTPS cases proved +1/query; the `Copy` `Other(u16)` redesign removes it (HTTPS cache-hit 704 → 640 = A); dev-box oracle + RB5009 controlled cache-hit experiment confirm. Code in the working tree pending the clean gate. See Finding 9 |
| 6 | Raise `intercept_alloc`'s `BATCHES` to 6 | **FIXED** `f51a830` | Removed the one-batch-of-evidence problem in Finding 7 |
| 7 | Measure Finding 3 with `diag-timing` | **CLOSED — measured, no code change** | `dispatch_wait_us` ≈ 62 µs, ~4.5% of the 1.389 ms miss; the inline-cache fast path was rejected on the number, not deferred. See Finding 3 |
| 8 | Add an oracle for the listener layer | OPEN | Closes the Finding 8 gap for `udp.rs`, `tcp.rs` and the F3 `splice` |
| 9 | Add a >64 KiB TCP reply test | OPEN | Pins the `tcp.rs:222` invariant to a test instead of a comparison in another file |
| 10 | Work the Finding 10 debt register down, one row at a time | ONGOING | The allocation-free invariant stands; §Measurements lists every surviving allocation, and each needs elimination or a measured justification. Items 3, 5 and 8 above are its first instalments |
| 11 | Measure F3 — the length-prefix framing at `tcp.rs:223` | **CLOSED — measured, no code change** | 13–16 ns at real reply sizes, an order of magnitude under the 1 µs screening gate at every size; the realloc is a capacity edge, not a per-reply cost; the vectored alternative is slower below ~1 KiB. See §TCP length-prefix framing (F3) |

**What the three fixed rows changed.** Item 1 added
`a_shard_poisoned_by_a_panicking_sweep_still_serves_and_stores`, which fails on
a `PoisonError` without the fix — so Finding 1's "no test exists" note is
answered by that commit rather than by an edit here. Items 3 and 4 added no
test: a move versus a clone is enforced by the compiler, and asserting a log
level needs a tracing capture this workspace does not have. Both gaps are
named in their findings and both remain open.

**PASS WITH DEFERRED FINDINGS** — as taken: 1 high (2: ClientHello rescan —
**measured on the RB5009 and closed, no code change**; up to 32 drippers moved
p95 not at all, the 16 KiB cap neutralises the amplification),
1 medium (1: poisoned cache shard kills a shard's keyspace permanently and
invisibly — **fixed, `3ce7ec5`**), 6 low (3: `spawn_blocking` per DoT
connection — **measured on the RB5009 and closed, no code change**;
`dispatch_wait_us` ≈ 62 µs, the inline-cache fast path rejected on the number;
4: removable `CacheKey` clone — **fixed, `c93e2d1`**; 5:
unthrottled `error!` per connection — **fixed, `bb15de7`**; 6: two host copies
per DoT connection; 9: `QueryType::Other` allocates per query — **redesigned to a
`Copy` `Other(u16)`, validated on the RB5009, accepted**; the +1/query is gone
(HTTPS cache-hit 640 = A), code in the working tree pending the clean gate;
10: the hot path does not yet satisfy PERFORMANCE.md's
allocation-free invariant — performance debt, worked down per row),
2 informational (7: oracle ceilings pinned at zero margin — **fixed,
`f51a830`**; 8: listener layer has no oracle).

**Eight resolved, three open.** Fixed by a change: 1, 4, 5, 7. Closed by
measurement with no change: 2, 3, and the TCP length-prefix framing tracked as
TODO row 11 — F3 in the earlier review's numbering, which is not one of this
audit's ten findings, so the ten split 7 resolved / 3 open and the eighth is
that row. Redesigned and validated, commit pending: 9.
Still open: 6, 8, 10. The verdict stays PASS WITH DEFERRED FINDINGS because it
records the audit as taken; the outcomes are tracked in §Remaining TODOs and in
git, not by rewriting the findings.

No finding blocks — the one HIGH and the DoT dispatch were both measured down to
no action.

**Numbers are stable, so §Findings no longer reads in severity order.** Findings
2 and 9 were both raised on the documentation cross-check and kept their
original numbers rather than being renumbered, because they are referred to by
number elsewhere. Read the severity from each heading, not from its position.
F1 appears closed, F2 cited as already argued, F3 closed on measurement, F4 not
a code matter.

## Shit found by Fable

Read-only review of the five fix commits and the two measurement closures,
2026-09-16. Dev box, x86-64, `rustc 1.96.0`, debug profile. Evidence:
`cargo test -p fah-model -p fah-rules -p fah-stats -p fah-api -p fah-dns -p fah-http`
— 34 test binaries, 0 failures. No Rust comment added by any fix commit; the
bench's one added line is the `// SAFETY:` exception.

### Summary

- All five code fixes — `3ce7ec5`, `c93e2d1`, `bb15de7`, `f51a830`, `af5cf61`
  — are correct and match the finding each one closes. Nothing blocks.
- Two LOW residuals, five informational, two doc-drift rows.
- Finding 2 and Finding 3 were closed on the RB5009 and are not re-measured
  here. The reasoning holds on its own: the 16 KiB cap × `hello_timeout` ×
  `max_connections` bounds the rescan, and ~62 µs of a 1.389 ms miss does not
  buy a new lock reachable from an async worker.

### Findings

Severity-ranked.

#### S1 — LOW · a recovered shard keeps serving, but its byte count is no longer true

`crates/fah-dns/src/cache.rs:803` and `:810`; the same class at `:348`
(`insert`'s eviction loop).

**Failure scenario.** A panic mid-`retain` in `clean` unwinds the locals
`freed` and `removed`. The entries `retain` already dropped are gone from the
map, but `guard.bytes` is never decremented for them and `sweep_queue` never
runs. `Shard::bytes` over-counts for the life of the process, so
`over_bounds()` evicts earlier than the configured `byte_capacity` allows.
Bounded — hard rule 4 holds — and the cost is hit rate, not memory.

**Smallest fix.** `clean` already walks every entry; sum the surviving
entries' `entry_heap_bytes` in that walk and assign `guard.bytes` from it
instead of subtracting `freed`. Every sweep then self-heals any skew. Cost:
none on the query path; the sweep is already O(entries).

Finding 1's "Cost: none" stands. Its "recovers" reads better as "keeps
serving" — the guard is recovered, the invariant behind it is not re-checked.

#### S2 — LOW · `parse_qtype` coerces any unknown spelling to `TYPE0`

`crates/fah-api/src/wire.rs:302-307`.

`"qtype": "FOO"` or `"qtype": "TYPE99999"` on `POST /api/v1/rules/test`
becomes `QueryType::Other(0)` and is echoed back as `TYPE0`. Before `af5cf61`
the caller's text came back unchanged. Silent coercion, no `400`. API.md pins
nothing here, so it is an owner decision: reject with `400`, or document the
coercion.

#### S3 — INFORMATIONAL · an unreachable arm in `qtype_label`

`crates/fah-api/src/wire.rs:279`. `qtype_name` matches `Other` first, so the
`"OTHER"` arm never runs. Delete it (principle 14) or route `qtype_name`
through it.

#### S4 — INFORMATIONAL · the "last domain" error fires once per acceptor, not once

`crates/fah-http/src/server.rs:322`. `Rotation` is `Clone`, and
`crates/fah-http/src/tls_server.rs:47` takes its own copy via
`http.rotation()`. Each acceptor removes senders independently and each logs
the transition — up to two lines, bounded by the acceptor count. Finding 5's
purpose (no per-connection `error!`) is met. `http_domain = index` names a
rotation slot after removals, not a stable domain id — pre-existing.

#### S5 — INFORMATIONAL · the poison test installs a process-global panic hook

`crates/fah-dns/src/cache.rs:1851-1856`. Lib tests run in parallel; a test
that panics inside that window loses its message (it still fails). One user
in the crate today, so harmless until a second appears.

#### S6 — INFORMATIONAL · the F3 bench measures a copy of `frame_reply`

`crates/fah-dns/benches/frame_reply.rs:70` duplicates
`crates/fah-dns/src/tcp.rs:221`, which is `pub(crate)` and so unreachable
from a bench. A later change to `frame_reply` leaves the bench measuring the
old construct with no signal.

#### S7 — INFORMATIONAL · dashboard comments still describe `Other(name)`

`dashboard/frontend/src/pages/rule-tester/query-form.tsx:6-7` and
`dashboard/frontend/src/api/types.ts:771-772` say `parse_qtype` maps
everything but `A` and `AAAA` to `Other(name)`. Comment drift only; the chips
and the request shape are unaffected.

### Verified — nothing to add

| Commit | Checked | Result |
| ------ | ------- | ------ |
| `3ce7ec5` (F1) | `lock_shard` at all six sites; the test poisons a shard under `catch_unwind`, then asserts `lookup`, `store` and `clean` still work; `Ok` arm is the branch `unwrap` already took | correct |
| `c93e2d1` (F4) | `key` is dead after `offer`; `answer` is still borrowed after it; a move into a returning branch is compiler-enforced | correct |
| `bb15de7` (F5) | transition lines bounded by domain count; per-connection line at `debug!`; `send` exits on an empty rotation | correct, see S4 |
| `f51a830` (F7) | assertions still read the last two batches; warm-up now gets four | correct |
| `af5cf61` (F9) | `qtype_bit` returns 0 for `Other`, exactly what `rrtype_bit` returned for an unknown name, so negated `$dnstype` semantics are unchanged; the drift guard pins all 15 indices to `rrtype_bit`'s table; the live feed and the rule tester reach JSON through `qtype_name`, so the serde-derive shape (`{"Other":65534}`) is never on the wire; nothing persists `QueryType` via serde — snapshot and history store bucket indices; `forward_alloc` ceilings ratcheted 12→11, 11→10, 18→17 | correct, see S2 and S3 |
| `52eec09` (F3) | bench-only; no production file touched; the one added comment is `// SAFETY:` | correct, see S6 |

### Doc drift in this file

| Where | What | Proposed |
| ----- | ---- | -------- |
| lines 596, 1342, 1369, 1379 | "working tree pending" / "commit pending" — `af5cf61` landed 2026-09-16 | say so; put the hash in TODO row 5 |
| API.md, `qtype` field | the RFC 3597 `TYPE<n>` spelling exists only in this file | one sentence beside the field; CONTEXT.md §Record Type stays accurate, the eleven stats labels did not change |

**PASS WITH DEFERRED FINDINGS** — S1 and S2 are owner decisions; S3–S7 are
housekeeping. No fix commit is wrong.
