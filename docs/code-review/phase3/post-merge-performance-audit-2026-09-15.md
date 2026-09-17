# Audit — hot path, memory and Rust quality at `2b03a30`

Replaces the first pass of the same date (commit `2b03a30`), whose lock, panic
and oracle figures were wrong. The corrected counts are below.

Why the first pass undercounted, since it is not a finding about this code: the
audit recipe cut each file at its first `#[cfg(test)]`, and `cache.rs` carries
that attribute on two individual methods (`len` at `:675`, `queue_len` at
`:685`) long before the test module at `:862`. The cut therefore dropped 187
lines of production code — `note_lookup`, `clean`, `cleanup_stats`,
`positive_ttl`, `negative_ttl` — and with them 2 lock sites and 2 clock reads,
which is how a pass over files that hold locks reported "0 locks". Anchoring the
pattern at column 0, `grep -n '^#\[cfg(test)\]'`, fixes it; of the 13 files in
scope only `cache.rs` differs between the two patterns. The recipe lives in
chat, not in the repo, so nothing here can hold that fix.

Corrected 2026-09-17 after an independent re-read: A3's premise and status,
A6's account of what the hook commit changed, and three counts — the throttle's
test count, the memory delta, and the `udp.rs` classification list.

Every `file:line` here is as of `2b03a30`. A1 and A2 were fixed afterwards, so
the lines they name have moved — each of those two sections ends with what
shipped, and §Fix verification carries the evidence.

## Summary

- `base commit: e8e7cf8` · `head commit: 2b03a30` · `mode: SNAPSHOT`. The working
  tree is clean; `git diff --stat e8e7cf8..HEAD` touches one documentation file
  and no code, so there is no changeset to review.
- SNAPSHOT consequences: new state, new error paths, new `.await`s,
  `Send`/`Sync` changes, memory delta and before/after oracle numbers are
  **N/A — SNAPSHOT** and are not reported as zeros.
- Scope is the four hot-path entry points plus their one-level callees, 13
  files. Every count is production-only; the `#[cfg(test)]` tail is excluded.
- Six findings, none blocking, and **four are fixed**: A1 and A2, the two medium
  ones — an accept loop with no backoff, and four unrated `warn!` paths that are
  one defect in four places — plus A5 and A6, both documentation. Each entry
  keeps what was found and then states what shipped, including where the
  implementation departed from the proposal and why, and A6 carries two claims
  filed under it that are withdrawn. **Nothing is open.** A3 was deferred, then
  closed by measurement the same day (`52eec09`), and A4 was resolved
  procedurally — neither carries work.
- All five oracles pass, reported as observed / ceiling / headroom: 8 of 12
  ceiling checks clear by exactly the 4-allocation jitter allowance, so the
  ceilings equal today's measurements. A pass means no regression beyond the
  allowance; it does not mean four allocations are available to spend.
- Hot path holds otherwise: 0 `unsafe`, 0 `panic!`, 0 `Regex`, 0 std guards
  across `.await`, no memory retained per query or per request.

## Decisions

- Findings carry local `A` labels. Bare `F` numbers are taken: the global
  review registry already uses them for whole files
  (`phase2.6/f2-udp-inflight.md`, `phase2.6/f3-name-alloc-attribution.md`,
  `phase3/f7-flapping-oracle-redesign.md`), so the first pass's local F1–F4
  collided with them. F1 is A2 here, F2 is A5, F3 is A3, F4 is A6.
- `server.rs:181 accept_loop` is in scope even though the entry-point list names
  `tls_server.rs`: that file only spawns it. Both the HTTP and the HTTPS
  listeners run this one loop, and no earlier audit covered it.
- Correct as written, not findings: the shard-selected `std::sync::Mutex` in
  `cache.rs` (documented exception to hard rule 3 —
  [ARCHITECTURE.md](../../../ARCHITECTURE.md) §Runtime Model and the `cache.rs`
  header; A5 fixed the rule text, the code was never the problem);
  `tokio::sync::Mutex` at
  `intercept.rs:486` and `swr.rs:173`, both scoping the guard so the `.await`
  that matters runs outside it; `judge` building `ModelRequest` unconditionally,
  since `events` is `Some` on every production wiring path (`main.rs:588`,
  `main.rs:1127`).
- The oracles run under `cargo test`, i.e. the dev profile: `debug_assert!` is
  live and nothing is optimized. The numbers are regression detectors on this
  dev box, not production allocation counts for the RB5009. No bench was run and
  no regression claim is made — the A1/A2 fix touches only error paths, so there
  is nothing on a bench's success path for it to move.
- **The HTTP/HTTPS acceptor recovers rather than dying** (owner's decision,
  2026-09-15). `RetryPolicy::never_fatal()` there, `RetryPolicy::new()` and its
  ~33 s escalation unchanged for the three DNS listeners. Descriptor exhaustion
  clears on its own and routinely outlasts 33 s, so the signal is a throttled
  `warn` with a cumulative count, not a dead task reported through
  `record_task_death`.

## Bugs found

### A1 — the HTTP/HTTPS accept loop has no backoff; a persistent `accept` error spins

`crates/fah-http/src/server.rs:207-209`

```rust
            Err(err) => {
                tracing::debug!(error = %err, "accept failed");
            }
```

The loop acquires a permit, calls `accept`, logs, and loops with no sleep and no
fatal path. Under `EMFILE`/`ENFILE`/`ENOBUFS` — file-descriptor exhaustion on a
1 GB router is reachable — `accept` returns immediately and forever, so the task
burns a core and writes one `debug!` per iteration until descriptors free up.

All three DNS listeners already handle this: `udp.rs:141`, `tcp.rs:96`,
`dot.rs:81` each run `RetryPolicy` with a sleep and a `Fatal` exit.

Violates engineering principle 4 (shared behaviour in one place) and hard rule 4
in spirit — the log volume grows with uptime under a condition the loop cannot
end.

Severity: medium. No memory growth; CPU starvation of the DNS listeners on the
same runtime is the real cost.

**FIXED.** `RetryPolicy` moved from `fah-dns/src/backoff.rs` to
`fah-common/src/retry.rs` — hard rule 1 forbids `fah-http` importing `fah-dns`,
so the L1 move was the only non-duplicating route. `new()` keeps its exact
behaviour and its four tests came with it; `never_fatal()` is new. The DNS
listeners import the moved type and are otherwise untouched.

`accept_loop` now runs `RetryPolicy::never_fatal()`, releases the admission
permit before the sleep, and takes the `Fatal` arm as a sleep at the ceiling so
no future policy change can silently end the listener:

```rust
            Err(err) => {
                drop(permit);
                let delay = match policy.on_error() {
                    RetryDecision::Sleep(delay) => delay,
                    RetryDecision::Fatal => BACKOFF_MAX,
                };
```

Three implementation decisions that differ from what this section first
proposed, each for a reason found while writing the code:

| Proposed | Shipped | Why |
| -------- | ------- | --- |
| reuse `Fatal`, let supervision see the death | `never_fatal()`, the listener recovers by itself | owner's decision. `FATAL_CONSECUTIVE_ERRORS = 40` with the sleep capped at 1 s means Fatal after ~33 s, and descriptor exhaustion routinely outlasts that. A listener that can recover should recover; the throttled `warn` and its cumulative count are the signal instead |
| move `trait Accept` to `fah-common` and share it with `fah-dns` | a local `trait Accept` in `fah-http/src/server.rs` | not the same trait. `fah-dns`'s has an associated `Stream` type, which its own tests need for in-memory duplex streams; `fah-http`'s `Dispatch` hands `Accepted<TcpStream>` on, and `detach()` exists only for `TcpStream`. Sharing would have forced the dispatch generic for no gain. The failing test listener returns only `Err`, so a trait fixed to `TcpStream` costs it nothing |
| a throttle field on a gauge | a `LogThrottle` local to the loop frame | the failure happens in the loop itself, which lives exactly as long as the listener. No `Arc`, no struct field. A2's sites do need gauge ownership, because they log from spawned per-connection tasks |

One isolated failure still logs at `debug!`: a client that aborts between the
handshake and our call is ordinary. The `warn!` fires only once the backoff has
reached `BACKOFF_MAX`, i.e. the failure is sustained, and then at most once per
`ACCEPT_WARN_INTERVAL` (60 s).

### A2 — four `warn!` paths a client can drive, none rate-limited

One defect in four places. The listeners that *do* limit their log volume set
the contrast: the `recv`/`accept` paths run `RetryPolicy` (`udp.rs:141`,
`tcp.rs:96`, `dot.rs:81`) and the cleanup sweep is bounded by its interval
(`pipeline.rs:223`). The RouterOS log buffer is small — `pipeline.rs:236`
already reasons about exactly this cost.

| Site | Evidence | Rate |
| ---- | -------- | ---- |
| `udp.rs:176` | `warn!(error = %err, client = %client, "failed to send UDP DNS reply")` | one line per datagram |
| `response.rs:165` | `warn!(error = %err, "failed to encode DNS response; falling back to SERVFAIL")` | one line per query, every transport |
| `dot.rs:194` | `warn!(host = %logged, error = %err, "DoT leaf pre-warm task failed")` | one line per DoT connection |
| `tcp.rs:129` | `warn!(error = %err, client = %client, "{what} connection ended with an error")` | one line per connection |

Reachability differs. `udp.rs:176` needs only a client network that stops
accepting replies. `dot.rs:194` fires on `JoinError`, so a repeatable panic in
leaf minting gives one line per connection. `tcp.rs:129` is already classified —
`is_client_disconnect` (`tcp.rs:136`) sends the ordinary hang-ups to `debug!` —
and what is left is per connection. `response.rs:165` has unproven reachability;
the absence of a limit is not in question either way.

**The shared fix is the throttle, not the classification.** It belongs in
`fah-common` (L1, reachable from both `fah-dns` and `fah-http`) — engineering
principle 4; four hand-rolled counters would be the same logic copied four
times.

**Throttle on time, not on a count.** "Every Nth occurrence" keeps the log rate
proportional to the failure rate: at 10 kqps and N = 1024, `udp.rs:176` still
writes ~10 lines a second, which is the flood this finding is about. The rate
has to be independent of traffic. Shape:

```rust
pub struct LogThrottle {
    origin: Instant,
    last_millis: AtomicU64,
    total: AtomicU64,
    interval_millis: u64,
}

impl LogThrottle {
    pub fn new(interval: Duration) -> Self;
    pub fn note(&self, now: Instant) -> Option<u64>;
    pub fn total(&self) -> u64;
}
```

`note` increments `total`, then attempts a `compare_exchange` on `last_millis`;
the one thread that wins emits, and the line carries the cumulative total. The
counter is never reset, which removes cross-window attribution and the
off-by-one over whether the emitting event counts itself. A reader who wants a
rate subtracts two lines.

The contract, which has to be settled before the code is written:

| Question | Answer |
| -------- | ------ |
| what the number means | cumulative events since process start, **best-effort telemetry** — a line is monotonic, but it is not a perfect snapshot of every concurrent event that landed just before it. Not an audit count |
| does the line include its own event | yes: the emitting thread logs the value its own `fetch_add` returned, not a fresh `load` |
| time origin | `origin: Instant` captured in `new()`; `note(now)` converts `now` to millis since it. This costs the `const fn` — `Instant::now()` is not const |
| "never logged" | sentinel `last_millis == 0`, so the stored value is `elapsed_millis + 1` and real time zero is 1 |
| why not `Mutex<Option<Instant>>` | simpler to reason about, but it puts a lock on an error path a remote can drive — contention exactly when things are failing |

The clock arrives as a parameter, which keeps `note` pure: the test feeds it
fabricated instants and needs neither a paused runtime nor a subscriber that
captures lines. Cost: two atomics and an `Instant` per instance, no allocation,
no lock, and nothing on the success path — the counter only moves on an error.

**Ownership decides whether the throttle works at all.** A per-connection
instance is not a throttle: a client that opens many connections gets one line
each and the interval means nothing. Every instance must be reachable through an
`Arc` owned by listener-scoped or process-scoped state, and nothing held
by-value in a `Clone` type — `DotTls` (`dot.rs:30-34`) is `Clone` and is cloned
once per connection, so a throttle field there would be exactly the defect.

| Site | Owner | Scope |
| ---- | ----- | ----- |
| `udp.rs:176` | `UdpInflightGauge` (`udp.rs:20-25`) | one per listener; it already holds `shed`, a counter of the same kind |
| `response.rs:165` | `Pipeline` | one per process — `udp::run`, `tcp::run` and `dot::run` all take the same `Arc<Pipeline<F>>` |
| `dot.rs:194` | the DoT connection gauge | one per listener |
| `tcp.rs:129` | the TCP connection gauge | one per listener, already in scope where `report_connection_end` is called |

`DotConnectionGauge` is an alias of `TcpConnectionGauge` (`dot.rs:24`,
`tcp.rs:27`), so one field on that struct serves both sites — with separate
instances, one per listener. A client using both transports therefore earns two
lines per interval rather than one. Deliberate: the gauges are per listener, and
merging them would couple the two listeners for nothing.

No `static` anywhere — hard rule 10 forbids the hidden state.

**Classification stays per site**, because the error kinds differ:

- `udp.rs:176` — `is_client_disconnect` does **not** transfer here. It matches
  `BrokenPipe`, `ConnectionReset`, `ConnectionAborted` and `UnexpectedEof`,
  which are stream errors UDP does not produce. The kinds that belong at
  `debug!` are `HostUnreachable`, `NetworkUnreachable` and `PermissionDenied`;
  everything else keeps its `warn!`, because a reply that will not leave the
  host is our own bug. This list was measured rather than reasoned about — see
  §UDP send errno probe, which removed a fourth kind and corrected one claim
  made here.
- `response.rs:165` — an encode failure is always ours, so nothing is demoted.
  This site needs the log moved, not throttled in place: `encode` is a free
  function in a pure-logic module, so giving it a throttle means either a
  `static` or a parameter that couples the module to logging. Return the
  fallback to the caller and let `Pipeline` log it — engineering principle 6,
  the library holds the logic and the caller owns the effects.
- `dot.rs:194` — a `JoinError` is either a panic or a cancellation; the panic
  case is ours and stays at `warn!`.
- `tcp.rs:129` — classification already correct; throttle only.

Severity: medium, carried by `udp.rs:176`, which an unauthenticated remote can
sustain. The other three are low on their own. No memory growth anywhere; log
history loss only.

**FIXED.** `LogThrottle` shipped in `fah-common/src/throttle.rs` exactly as the
contract above specifies, with eight tests: the first event always logs, events
inside the interval are suppressed, the boundary logs again with the cumulative
total, the total is never reset so successive lines only grow, suppressed events
still count toward the next line, a zero interval logs everything, real time
zero is not mistaken for "never logged", and eight threads driving 2000 events
produce exactly one line and lose no count.

Per site:

| Site | What shipped |
| ---- | ------------ |
| `udp.rs` | `is_client_unreachable` demotes `HostUnreachable`, `NetworkUnreachable` and `PermissionDenied` to `debug!` and returns (`ConnectionRefused` was in the first version; §UDP send errno probe removed it); everything else goes through `UdpInflightGauge::send_failures`. `handle_datagram` now takes `&Listener<S>` rather than `&S`, which is how it reaches the gauge |
| `response.rs` | `encode` returns `Encoded { bytes, failure: Option<ProtoError> }` and logs nothing. `Pipeline::encoded` logs through `encode_failures` and hands the bytes on; the three call sites in `handle` route through it |
| `tcp.rs` | `report_connection_end` takes `&TcpConnectionGauge` and throttles the non-disconnect arm. The call sites pass `&open` — `OpenConnection` derefs to the gauge, so nothing new is threaded through |
| `dot.rs` | `tcp::report_prewarm_failure` throttles the `JoinError` arm through the same gauge. `prewarm` takes `&DotConnectionGauge`, passed down from `serve_connection`, which already had it |

`TcpConnectionGauge` gained two throttle fields, `connection_errors` and
`prewarm_failures`, and a hand-written `Default` — `LogThrottle` has none, and
inventing one would have meant inventing an interval. Because
`DotConnectionGauge` is an alias, both DNS-over-stream listeners get the fields
with one change and keep separate instances.

Classification is tested as a pure function, not through the log: the three
unreachable kinds must demote; `InvalidInput`, `Other`, `BrokenPipe`,
`ConnectionReset` and `UnexpectedEof` must not; and since the errno probe
`ConnectionRefused` must not either, in a test named for the reason.

### A3 — one extra allocation per TCP/DoT reply, and the obvious fix is invalid

`crates/fah-dns/src/tcp.rs:183`

```rust
    reply.splice(0..0, len);
```

`encode` returns `message.to_vec()` (`response.rs:164`). This section first
called that an exactly-sized `Vec`; it is not — hickory starts the buffer at
`Vec::with_capacity(512)` (`hickory-proto-0.26.1/src/op/message.rs:503`), so
prepending the two length bytes is a memmove on every reply and a realloc only
when the reply lands exactly on the capacity. Engineering principle 3. TCP/DoT
only, not UDP.

**Do not encode at a two-byte offset.** `BinEncoder::with_offset` moves the write
cursor, and `name_pointers` stores absolute buffer indices that are emitted
straight into the message
(`hickory-proto-0.26.1/src/serialize/binary/encoder.rs:114-123`, `:260`,
`:273`), so a message starting at index 2 carries every name-compression pointer
two bytes too high — an invalid reply. The three fixes that would work each cost
more than the realloc: a vectored write (tokio has no `write_all_vectored`, so
the loop is hand-rolled), a `BufWriter` per connection (~8 KiB × connections),
or a framing buffer reused per connection (amortizes the realloc, keeps the
`encode` allocation).

Severity: low. Small cost, no correct small fix.

**DEFERRED — measure before optimizing. Revisit if DoT becomes a dominant
transport.** Owner's decision, 2026-09-15, superseded the same day by the
closure below. Nothing is owed and no work is
tracked: the reasoning is that the obvious fix is invalid, the three valid ones
each cost more than the realloc, and TCP/DoT are not the dominant transports
today. The reopening criterion is concrete — Android Private DNS is DoT, so
household phones moving to it would make DoT the main path and change the
premise.

**CLOSED the same day, by measurement.** Commit `52eec09` benched the framing
at 13–16 ns at real reply sizes, an order of magnitude under the 1 µs screening
gate, with the vectored alternative slower below ~1 KiB; the figures and the
corpus are in [hot-path-audit-dns-http.md](hot-path-audit-dns-http.md) §TCP
length-prefix framing (F3). No code change, and the DoT reopening criterion no
longer applies — the cost is settled, not deferred.

### A4 — the oracle ceilings equal today's measurements, so a pass carries no margin

`crates/fah-dns/tests/forward_alloc.rs:276`,
`crates/fah-http/tests/proxy_alloc.rs:246`,
`crates/fah-http/tests/intercept_alloc.rs:329`

```rust
            measured[1].0 <= REQUESTS * case.ceiling_per_request + JITTER_ALLOWANCE,
```

In 8 of 12 ceiling checks the margin is exactly `JITTER_ALLOWANCE` — 4
allocations over 64 queries, 0.06 per query. The ceilings were set to what the
code measured, so these are tight regression detectors: one extra allocation per
query or request fails them immediately, which is what they are for. Reporting
them as "passed" invites the opposite reading — that there is room.

Severity: low. Method, not code.

**RESOLVED PROCEDURALLY, not tracked as work.** Owner's decision, 2026-09-15:
this is not a code item and gets no task. From here on every oracle citation
carries three numbers in one place, observed / ceiling / headroom — `832 / 836 /
4` — and the interpretation is stated rather than left to the reader:

> PASS means no regression beyond the measurement allowance; it does not mean
> four allocations are available to spend.

The oracle tables below already follow it.

### A5 — hard rule 3 forbids locks on the hot path; the cache takes one per query

`crates/fah-dns/src/cache.rs:46`, `cache.rs:519`

Design, not defect — a shard-selected `std::sync::Mutex`, argued in the file
header and in [ARCHITECTURE.md](../../../ARCHITECTURE.md) §Runtime Model. The
rule as written is contradicted by the shipped design.

Severity: low. Documentation only.

**FIXED.** Hard rule 3 in [CLAUDE.md](../../../CLAUDE.md) now reads "no
allocations, no regex, and no locks the architecture does not already name",
followed by "a new hot-path lock needs an ADR and its own figure in a
measurement file" and a pointer to ARCHITECTURE.md §Runtime Model.

Two wordings were rejected on the way, and the reasons are the useful part.
**Rewording the rule around contention** — "no *contended* locks" — describes
the shipped design more truthfully, since what rule 3 protects is the absence of
blocking and contention rather than the absence of the word `Mutex`. It was
dropped because it turns a greppable rule into a judgement, and a judgement is
resolved by the agent applying it: every lock looks uncontended to the agent
adding it. **"No *unjustified* locks"** fails the same way — justification is a
paragraph anyone can write. The shipped text replaces judgement with two
artifacts that exist or do not: an ADR, and a figure in a measurement file.

The canonical text lives in `CLAUDE.md` only. Six other files reference rule 3;
none restates it, so there is no second copy to drift — the failure that put
rules 19 and 20 in `plan/CLAUDE.md` where nobody read them.

Removing the lock instead was considered and declined. It needs immutable
entries swapped by compare-and-swap, the refresh claim moved into an atomic in
the entry, and a lock-free eviction queue — the last being the real work, since
the alternative, probabilistic eviction, changes the cache's behaviour and not
just its implementation. It also brings epoch-based reclamation, which defers
frees and so pushes against hard rule 4. All of that to save a CAS and an atomic
store on a path where a miss waits on the network. Principles 8 and 16.

The measurement that would reopen this, and it is cheap: count `try_lock`
failures per shard under household traffic. No failures over a week means no
contention and nothing to remove — and it produces exactly the figure the new
rule text asks for.

### A6 — the no-comments hook cited a rule number that does not exist

`.claude/hooks/no-rust-comments.sh:3` said it enforced "hard rule 20";
`CLAUDE.md` numbers that rule 7. The hook was written against
`plan/CLAUDE.md`'s old copy of the principles, which had a rule 19 and a rule
20, and the number was left behind when the list was consolidated into the root
file. Only a reader who opens the script sees it — the block message cites no
rule number.

Severity: low. One line, no behaviour.

**FIXED.** The reference is now by name rather than by index, so reordering the
list cannot break it again, and the line records why the old number was wrong.
The same commit (`3e97d16`) went further than this finding asked: the gate now
covers the dashboard's `.ts`/`.tsx` files as well, single-line template
literals are stripped before the scan so a `${scheme}//${host}` URL is not read
as a comment, and `no-rust-comments.test.sh` pins 14 cases. CLAUDE.md hard rule
7 names the TypeScript gate as of 2026-09-17; until then the hook enforced a
rule no document stated. Re-verified after the edit: `// nope` is blocked with
`exit 2`, and the exemptions still pass — a URL inside a string literal,
`// SAFETY:`, and any path outside `.rs`, `.ts` and `.tsx`.

**Two earlier claims under this number are withdrawn.**

The first was that hard rule 7 "does not describe the tree", on the evidence of
8369 comment lines under `crates/**/*.rs`. That is not a defect. The rule is a
prohibition on adding comments, not a description of what the tree contains; the
existing comments predate it. Presenting the count as a finding framed the rule
itself as the thing needing a decision, which it is not — comments are an input
cost paid on every read of a file, by every agent, in every session, against a
one-time benefit.

The second was that the hook is too strict, because it rejects an edit whose new
text merely carries a pre-existing comment through unchanged. A guard is useful
because it is blunt. Comparing added comment lines against removed ones turns a
rule into a judgement — an agent could reword a comment while "moving" it — and
complexity inside a guard is a hole in the guard. A false rejection costs the
agent one retry; a false pass costs the tree exactly what the rule exists to
prevent. The case offered as evidence was mis-attributed too: a struct that
landed between a doc comment and its function came from a badly chosen edit
anchor, not from the hook.

## Measurements

### Scope — entry points and one-level callees

| Entry point | File, production lines | Callees audited |
| ----------- | ---------------------- | --------------- |
| `Pipeline::handle` | `fah-dns/pipeline.rs` 1-523 | `response::{max_udp_payload,error,blocked,from_cache,encode_for_transport}`, `qtype::{domain_of,to_fah_query_type}`, matcher `context_for`/`lookup_in`/`rewrite`/`decisive_rule`, `cache::{key,lookup,lookup_and_claim_refresh,store,note_lookup}`, `swr::{offer,note_deduplicated}`, `forwarder.forward`, `events.try_send` |
| DNS UDP accept | `fah-dns/udp.rs` 1-179 | `gauge.admit`, `Pipeline::handle`, `socket.send_to`, `RetryPolicy` |
| DNS TCP accept | `fah-dns/tcp.rs` 1-197 | `Semaphore::acquire_owned`, `OpenConnection::enter`, `handle_connection`, `frame_reply`, `report_connection_end`, `is_client_disconnect` |
| DoT accept | `fah-dns/dot.rs` 1-246 | `LazyConfigAcceptor`, `prewarm` → `spawn_blocking(store.prewarm)`, `tcp::handle_connection`, `tcp::report_connection_end` |
| HTTP request | `fah-http/proxy.rs` 1-704 | `judge`, `emit`, `request_host`, `strip_hop_by_hop`, `to_client_response`, `refuse`, `resolver.resolve` |
| HTTPS/SNI | `fah-http/sni.rs` 1-264, `tls_server.rs` 1-100 | **`server.rs:181 accept_loop`** (1-294), `Dispatch::dispatch`, `Accepted::{serve,detach,register}`, `Rotation::send` |
| Intercept | `fah-http/intercept.rs` 1-561 | `prewarm`, `Upstream::send`, `sender.lock().await` |
| Per-query callees | `cache.rs` 1-861, `swr.rs` 1-234, `response.rs` 1-175, `qtype.rs` 1-31 | — |

### Allocations — per-query, per-request, per-connection

| Site | Construct | Frequency | Verdict |
| ---- | --------- | --------- | ------- |
| `qtype.rs:27` | `String::with_capacity(name.len())` | per query | one sized alloc, lowercased in place |
| `qtype.rs:28` | `write!` into that buffer | per query | no second alloc |
| `qtype.rs:19` | `other.to_string()` | per query, unknown qtype only | `QueryType::Other` needs the name |
| `cache.rs:485` | `domain.into()` | per query | the cache key; pre-lowercased by the caller |
| `udp.rs:157` | `buf[..len].to_vec()` | per datagram | moved into the spawned task |
| `tcp.rs:164` | `vec![0u8; len]` | per message | `len` capped at `MAX_MESSAGE_LEN` (16 KiB) |
| `tcp.rs:183` | `reply.splice(0..0, len)` | per TCP/DoT reply | memmove; realloc only at the 512-byte capacity edge — **A3**, closed by measurement |
| `response.rs:70-129` | 10 × query / record / name clone | per synthesized reply | hickory owns its records |
| `response.rs:164` | `message.to_vec()` | per reply | wire buffer, 512-byte initial capacity |
| `cache.rs:600,614` | `answers.clone()`, `authorities.clone()` | per store (miss) | the cached answer itself |
| `cache.rs:630,640` | `Arc::new(answer)`, `key.clone()` | per store | `Arc` so lookups clone a pointer |
| `pipeline.rs:442` | `key.clone()` | per claimed stale refresh | handed to a bounded queue |
| `proxy.rs:483,484` | `vec![ip]`, `claim.host.clone()` | per request | resolve path; literal-IP hosts skip the clone |
| `proxy.rs:579,584,613` | 3 × `to_string()` | per request | consumed by `ModelRequest`; `events` is always `Some` in production |
| `proxy.rs:692` | `Vec<HeaderName>` collect | per request with a `Connection` header | the names come out of the header being removed; the +2/request the oracle shows |
| `intercept.rs:135` | `host.to_string()` | per connection | moved into `spawn_blocking` |
| `intercept.rs:151,209,216` | `Arc::new`, `Mutex::new` | per connection | the session's own state |
| `intercept.rs:269,270,502,546,553` | verdict / policy / sender / authority / host clones | per request | h2 needs its own sender; headers are rebuilt |
| `sni.rs:147` | `String::from_utf8_lossy` | per rejected SNI, only when `debug` is enabled | `tracing` evaluates fields lazily |
| once per process | `pipeline.rs:144,147,148,154,175`, `cache.rs:445,454`, `swr.rs:96,160`, `proxy.rs:278,280`, `tls_server.rs:36,37` | 13 sites | construction |

Totals: 26 per-query / per-request / per-connection sites itemized, 13
once-per-process. One finding (A3); nothing retained.

### Locks

| Site | Kind | Frequency | Verdict |
| ---- | ---- | --------- | ------- |
| `cache.rs:519` | `std::sync::Mutex`, shard-selected | per query | A5, documented exception |
| `cache.rs:626` | same | per store | A5 |
| `cache.rs:555,566` | same | per refresh job | A5 |
| `cache.rs:719,778` | same | per stats read / per sweep | off the query path |
| `swr.rs:155` | `std::sync::Mutex` | once per process | guard is a temporary, dropped at the statement |
| `swr.rs:173` | `tokio::sync::Mutex` | per refresh job | held across `recv().await` only; scoped before the forward |
| `intercept.rs:486` | `tokio::sync::Mutex` | per intercepted request | h2 clones the sender and drops the guard; H1 is serial by design |
| `tls_server.rs:36`, `server.rs:188` | `Semaphore` | per connection | admission control, not mutual exclusion |
| `pipeline.rs`, `udp.rs`, `tcp.rs`, `dot.rs`, `proxy.rs`, `sni.rs`, `response.rs`, `qtype.rs` | — | — | 0 sites |

### Syscalls, clocks and thread hops

| Site | What | Frequency | Verdict |
| ---- | ---- | --------- | ------- |
| `pipeline.rs:334` | `Instant::now` | per query | latency for the event |
| `pipeline.rs:389` | `SystemTime::now` | per query | event timestamp |
| `cache.rs:523,627,704,768` | `Instant::now` | per lookup / store / stats / sweep | freshness needs a clock |
| `dot.rs:117` | `Instant::now` | per DoT connection | handshake deadline — not feature-gated |
| `dot.rs:188` | `spawn_blocking(store.prewarm)` | per DoT connection | leaf minting is CPU-bound; must not block the runtime |
| `intercept.rs:136` | `spawn_blocking(store.prewarm)` | per intercepted connection | same reason |
| `intercept.rs:150,297` | `Instant::now` | per connection / per request | deadlines |
| `proxy.rs:371,616` | `Instant::now`, `SystemTime::now` | per request | latency and event timestamp |
| `pipeline.rs:217` | `spawn_blocking(cache.clean)` | per sweep interval | bounded by the interval |
| feature-gated | `dot.rs` `diag-timing` clocks | off in release builds | — |

No `fs::`, `File::`, `env::var`, `rand::` or `std::thread` on any hot path:
0 sites.

### Regex and formatting

| Site | What | Frequency | Verdict |
| ---- | ---- | --------- | ------- |
| `qtype.rs:28` | `write!` into a preallocated `String` | per query | no allocation beyond `:27` |
| `proxy.rs:202` | `format!` | per upstream-config error | not on the request path |
| — | `Regex` | — | 0 sites, hard rule 3 holds |

### Error and observability pressure

| Site | Per occurrence | Rate limit | Verdict |
| ---- | -------------- | ---------- | ------- |
| `udp.rs:176` | one `warn` per datagram | none | **A2** |
| `response.rs:165` | one `warn` per query | none | **A2** |
| `dot.rs:194` | one `warn` per DoT connection | none | **A2** |
| `tcp.rs:129` | one `warn` per connection | classification only | **A2** |
| `server.rs:208` | one `debug` per loop iteration | none, and the loop does not sleep | **A1** |
| `udp.rs:143`, `tcp.rs:99`, `dot.rs:83` | one `warn` per retry | `RetryPolicy` backoff + `Fatal` exit | bounded |
| `pipeline.rs:223` | one `warn` per failed sweep | the sweep interval | bounded |
| 40 further sites | `debug!` / `trace!` | level-gated, fields evaluated lazily | acceptable |

### Memory

**new state** — N/A — SNAPSHOT.

**lifetimes**

| Guard | Dropped by | Happy | Error | Timeout | Abort |
| ----- | ---------- | ----- | ----- | ------- | ----- |
| `Admitted` (`udp.rs:66-69`) | `Drop` → `gauge.release()` | yes | yes | n/a | yes, unwind drops it |
| `OpenConnection` (`tcp.rs:111`, `dot.rs:98`, `server.rs:199`) | explicit `drop` + `Drop` | yes | yes | yes | yes |
| `OwnedSemaphorePermit` (`tcp.rs:87`, `server.rs:188`) | moved into the task, dropped at its end | yes | yes | yes | yes |
| `Accepted{permit,open}` (`domain.rs:42-43`) | bound inside the async block | yes | yes | yes | yes — dropping the future returns both |
| DoT `_slot` (`dot.rs:101`) | task scope | yes | yes | yes | yes |
| SWR refresh claim (`cache.rs:511`) | `swr::offer` returns it on `Full` / `Closed` (`swr.rs:118-128`) | yes | yes | lease expiry | see cancellation |
| upstream driver tasks (`intercept.rs:392,404`) | complete when the sender is dropped | yes | yes | yes | detached, not aborted |

**cancellation**

| `.await` | If cancelled there | Verdict |
| -------- | ------------------ | ------- |
| `tcp.rs:146,167` `timeout(read_exact)` | a partially consumed message; the code returns `Ok(())` and closes | intended, RFC 7766 §6.2.4 |
| `dot.rs:119,147` `timeout_at` handshake | the stream is dropped, permit and gauge slot return | clean |
| `dot.rs:188`, `intercept.rs:136` `spawn_blocking(...).await` | the blocking task runs to completion and its result is discarded; the minted leaf still lands in the store | bounded by the blocking pool; no leak |
| `pipeline.rs:442` window between `lookup_and_claim_refresh` and `offer` | the claim is never handed back and sits out `refresh_claim_lease` | bounded by the lease, not leaked; the key serves stale meanwhile |
| `server.rs:201` `dispatch(...).await` → `Rotation::send` | `Accepted` is dropped, returning permit and gauge slot | clean |
| `intercept.rs:486` `sender.lock().await` | the guard is never taken; no state is half-written | clean |

**accumulation and key-space bounds**

| Structure | Bound | Test that fails if removed |
| --------- | ----- | -------------------------- |
| `cache.rs:393` shards | capacity + LRU queue | `the_cache_evicts_the_least_recently_used_host_at_capacity` |
| `swr.rs:93` queue | `workers * QUEUE_DEPTH_PER_WORKER` — configuration, not traffic | the queue-bound test under refresh churn (`queue_len`) |
| `fah-certs/leaf.rs:214` | capacity + LRU eviction | `the_cache_evicts_the_least_recently_used_host_at_capacity` |
| `fah-stats/client_registry.rs:120` | `while len >= capacity` + eviction | `capacity_evicts_the_least_recently_seen_client` |
| `fah-api/password.rs:109-113` | `max_tracked`, expired dropped first | the `per_address.len() <= 128` assertion at `password.rs:505` |
| `intercept.rs:97` `hello` | `MAX_HELLO_BYTES` (16 KiB) × `max_connections` | the admission test |
| `tls_server.rs:36`, `server.rs:188` permits | `max_connections` | — |

**memory delta** — N/A — SNAPSHOT.

### Rust quality

| Category | Sites | Verdict |
| -------- | ----- | ------- |
| `.lock().unwrap()` | `cache.rs:519,555,566,626,719,778`; `swr.rs:155` | 7 production sites. Poisoning needs a panic under the guard; the guarded regions do `HashMap` / `VecDeque` work, `Arc::clone` and `Instant` arithmetic on values already range-checked (`cache.rs:524`, ttl clamped at `cache.rs:845`). Unreachable unless a guarded section starts panicking, and then that shard is dead for the rest of the process |
| `debug_assert!` | `cache.rs:99,335,480` | dev profile only. `:480` scans the domain for uppercase once per query, so the dev-profile oracle numbers include it; release drops all three |
| `.unwrap(` / `.expect(` / `panic!` / `unreachable!` elsewhere | 0 in the other 11 files | holds |
| `unsafe` | 0 across all 13 files | hard rule 7's `// SAFETY:` exception is unused here |
| std guard across `.await` | 0 | `cache.rs` has no `async fn` in production; `swr.rs:155` drops the guard at the statement |
| `Send` / `Sync` changes | N/A — SNAPSHOT | — |
| indexing, casts, arithmetic | `cache.rs:524` `as u32` after a `<` check; `tcp.rs:182` `u16::try_from(...).unwrap_or(u16::MAX)`, unreachable under the 16 KiB cap; `udp.rs:157` `buf[..len]` with `len` from `recv_from` | no unchecked arithmetic on a hot path |

### Oracles — run at `2b03a30`, dev profile, x86_64 dev box

`forward_alloc`, 64 handles per case, `JITTER_ALLOWANCE = 4`:

| Case | Observed | Ceiling (per query → total) | Headroom |
| ---- | -------- | --------------------------- | -------- |
| blocked, inline name (each of Udp / Tcp / Dot / Doh) | 832 | 13 → 836 | 4 total (0.06/query) |
| blocked, heap name (each transport) | 1216 | 19 → 1220 | 4 total |
| cache hit, inline name (each transport) | 640 | 10 → 644 | 4 total |
| cache hit, heap name (each transport) | 1024 | 16 → 1028 | 4 total |
| miss, inline name Udp / Tcp / Dot / Doh | 1054 / 1034 / 1031 / 1027 | 17 → 1092 | 38 / 58 / 61 / 65 |
| miss, heap name Udp / Tcp / Dot / Doh | 1476 / 1476 / 1472 / 1472 | 24 → 1540 | 64 / 64 / 68 / 68 |
| warm adaptive forwards | 1216 then 1216 | steadiness only, no ceiling | n/a |

`proxy_alloc`, 64 requests, `JITTER_ALLOWANCE = 4`:

| Case | Observed | Bytes | Ceiling | Headroom |
| ---- | -------- | ----- | ------- | -------- |
| pass-through GET | 3264 | 2 021 696 | 51 → 3268 | 4 total |
| pass-through GET + `Connection` | 3392 | 2 030 528 | 53 → 3396 | 4 total |
| blocked script | 1280 | 1 178 176 | 20 → 1284 | 4 total |
| blocked document | 2048 | 1 316 992 | 32 → 2052 | 4 total |

`intercept_alloc`, 64 requests:

| Case | Observed | Ceiling | Headroom |
| ---- | -------- | ------- | -------- |
| intercepted pass-through GET | 3072 | 50 → 3204 | 132 total (2.06/request) |
| intercepted blocked script | 1600 | 25 → 1604 | 4 total |
| intercepted blocked document | 2432 | 38 → 2436 | 4 total |

`url_lookup_alloc`: `an_http_lookup_allocates_nothing_whatever_it_decides` — 0
allocations, pass. `dedup_alloc_bound`:
`with_capacity_bounds_the_transient_allocation_under_an_adversarial_ceiling` —
pass.

Reading: 8 of 12 ceiling checks clear by exactly the jitter allowance, so the
ceilings are today's measurements (A4). Superseded by re-running the same
commands; the dev profile and this box are part of the result.

### Fix verification — A1 and A2

Gates, whole workspace: `cargo fmt --all -- --check` clean,
`cargo clippy --workspace --all-targets -- -D warnings` clean,
`cargo test --all-features --workspace` green.

Oracles re-run after the fix, same commands and same dev box as above:

| Case | Before | After | Ceiling |
| ---- | ------ | ----- | ------- |
| blocked, inline / heap name | 832 / 1216 | 832 / 1216 | 836 / 1220 |
| cache hit, inline / heap name | 640 / 1024 | 640 / 1024 | 644 / 1028 |
| miss, inline name Udp / Tcp / Dot / Doh | 1054 / 1034 / 1031 / 1027 | 1054 / 1034 / 1036 / 1025 | 1092 |
| miss, heap name Udp / Tcp / Dot / Doh | 1476 / 1476 / 1472 / 1472 | 1478 / 1476 / 1474 / 1472 | 1540 |
| `proxy_alloc`, `intercept_alloc`, `url_lookup_alloc`, `dedup_alloc_bound` | pass | pass | — |

The blocked and cache-hit cases are byte-identical, which is the meaningful
comparison: they are the deterministic ones. The miss cases moved between −2 and
+5 allocations over 64 queries, inside their own spread, and every case is still
under its ceiling. Nothing was added to a success path: the new clock reads and
atomics sit on error paths only.

A1's test is falsifiable and was shown to fail both ways before being reverted:

| Wiring broken | Result |
| ------------- | ------ |
| `drop(permit)` removed | `available_permits()` asserts `left: 0, right: 1` |
| `tokio::time::sleep(delay)` removed | 10240 accept attempts where the passing run makes 2048 |

Memory delta: six process-lifetime `LogThrottle` instances — one on the UDP
gauge, two on each of the TCP and DoT gauges, one on the pipeline — of 40 bytes
each on Linux (`Instant` is 16 bytes there; 32 bytes on the Windows dev box),
plus one on each HTTP/HTTPS accept loop's frame. Far below the <1 MB threshold.

### UDP send errno probe — 2026-09-15

A2's classification listed four `ErrorKind`s chosen by reasoning about POSIX
errno, and its test asserted that reasoning back to itself without touching a
socket. `crates/fah-dns/examples/udp_send_errno.rs` measures it instead: it
drives `send_to` on an **unconnected** `UdpSocket`, the kind `udp::run` binds,
and prints the raw errno beside the `ErrorKind`.

Run on Linux 6.18 under WSL2, x86_64, static musl, cross-linked from the dev box
with `rust-lld`:

| Case | Result |
| ---- | ------ |
| unconnected, closed port, 1st / 2nd / 3rd send | `Ok(64)` every time |
| **connected**, closed port, 2nd send | errno 111 → `ConnectionRefused` |
| unconnected, broadcast without `SO_BROADCAST` | errno 13 → `PermissionDenied` |
| unconnected, 70 000-byte payload | errno 90 → `Uncategorized` |
| unconnected, IPv6 `2001:db8::1` with no route | errno 101 → `NetworkUnreachable` |
| unconnected, `0.0.0.0:53` and `240.0.0.1:53` | `Ok(64)` |

Every row is consistent with one reading, offered as the explanation of these
observations rather than as a law: errors the local kernel decides synchronously
surface on `send_to`, while errors delivered later by ICMP do not unless the
socket is connected. Routing and permission failures are the former,
port-unreachable is the latter.

Two corrections followed, both shipped:

- **`ConnectionRefused` is removed from the classifier.** It **was not produced
  by the unconnected `send_to` path FAH uses, under the tested Linux
  environment**; the probe showed it only for a connected UDP socket. Three
  sends to a closed port returned `Ok`, and only the connected socket saw errno
  111. `udp::run` never connects, so the branch had nothing to catch here.
  Engineering principle 14 — a branch no observation can reach is deleted, not
  kept for safety. The test that replaces it is named for the reason, since the
  code cannot carry one:
  `a_refused_port_is_not_classified_because_this_socket_is_never_connected`.
- **This document claimed `MessageSize` stays at `warn!`.** EMSGSIZE maps to
  `Uncategorized`, not to any named kind. The behaviour was right by accident —
  an unnamed kind is not in the demotion list, so it keeps its warning — but the
  claim named a variant the probe does not produce.

`HostUnreachable` was **not observed** and is kept on plausibility: no case here
produced errno 113. It stays classified because the mechanism that yields
`NetworkUnreachable` is the same route-lookup failure, and neither can be an
ICMP artefact. Treat it as unconfirmed rather than measured.

Scope of the result: measured on one host, one kernel and one architecture. The
errno → `ErrorKind` mapping is Rust's own `decode_error_kind`, which has no
architecture-specific step, and which errno the kernel picks is kernel logic
rather than architecture — so the same mapping is **expected** on aarch64/musl,
expected and not measured. The routing rows depend on this host's routing table
and should not be carried anywhere. The example is checked in so the container
can settle the ARM case on its own hardware whenever a probe runs there, which
is the only place that answer exists.

### Wiring test for the UDP send path

The classification and the throttle were each tested in isolation, and nothing
proved `handle_datagram` called them in the right order — an inverted condition
would have passed both. `Datagrams` is already a trait, so a stub whose
`send_to` returns a chosen kind now drives the real call site, and the
assertion reads `UdpInflightGauge`'s throttle count rather than captured log
output: unchanged for a demoted kind, incremented for a fault of ours.

Falsified by inverting the branch in `handle_datagram`, then reverted. The two
tests failed in opposite directions — `left: 1, right: 0` and `left: 0,
right: 1` — which is what distinguishes a wiring test from a restatement of the
code.

Gate after both changes: `cargo test --all-features --workspace` **1 651
passed / 0 failed**, up from 1 648.

### Self-check

| Check | Result |
| ----- | ------ |
| counts equal their rows | yes — 26 + 13 allocation rows, 7 lock sites, 8 `warn` sites |
| every scope file appears in every category, or is marked 0 | yes |
| no category reports 0 for something Decisions discusses | yes — the `intercept.rs` and `swr.rs` locks are rows, not zeros |
| every "bounded" names the test that would fail | yes, except the `swr` queue bound, whose test asserts `queue_len` rather than the queue depth directly |
| every finding carries file:line, evidence, rule, fix | yes |
| every number carries corpus, workload, device | yes — dev profile, x86_64 dev box, `2b03a30` |

## Files changed

The audit itself changed nothing. A1 and A2 then did:

| File | Change |
| ---- | ------ |
| `fah-common/src/retry.rs` | new — `RetryPolicy` moved here from `fah-dns`, plus `never_fatal()` and two tests for it |
| `fah-common/src/throttle.rs` | new — `LogThrottle` and its nine tests |
| `fah-common/src/lib.rs` | two module declarations |
| `fah-dns/src/backoff.rs` | deleted — moved to L1 |
| `fah-dns/src/lib.rs` | the `backoff` module declaration dropped |
| `fah-dns/src/udp.rs` | `is_client_unreachable`, the `send_failures` throttle on the gauge, `handle_datagram` takes the listener, two classification tests |
| `fah-dns/src/tcp.rs` | two throttle fields and a hand-written `Default` on the gauge, `report_connection_end` throttled, `report_prewarm_failure` added |
| `fah-dns/src/dot.rs` | passes the gauge into `prewarm`, both log sites routed through `tcp::` |
| `fah-dns/src/response.rs` | `Encoded { bytes, failure }`; the module no longer logs |
| `fah-dns/src/pipeline.rs` | `encode_failures` throttle and `Pipeline::encoded`, wired into the three encode sites |
| `fah-http/src/server.rs` | local `trait Accept`, `accept_loop` generic over it, never-fatal backoff, permit released before the sleep, throttled `warn`, and the falsifiable test |
| `fah-http/Cargo.toml` | tokio `test-util` in dev-dependencies, for the controlled clock |

A2's follow-ups then changed two more:

| File | Change |
| ---- | ------ |
| `fah-dns/examples/udp_send_errno.rs` | new — the errno probe, std only, runnable wherever the container runs |
| `fah-dns/src/udp.rs` | `ConnectionRefused` dropped from `is_client_unreachable`, one test renamed to carry the reason, and the `Datagrams` wiring test added |

A6 then changed two more, and the hook's behaviour with them:

| File | Change |
| ---- | ------ |
| `.claude/hooks/no-rust-comments.sh` | cites the rule by name instead of by number and records why the old number was wrong; the same commit extends the gate to `.ts`/`.tsx` and strips single-line template literals before the scan |
| `.claude/hooks/no-rust-comments.test.sh` | new — 14 cases: blocked, allowed and out of scope |

## Remaining TODOs

| Finding | Action | Severity | Owner decision |
| ------- | ------ | -------- | -------------- |
| A1 | **fixed** — see §Fix verification | medium | done |
| A2 | **fixed** — see §Fix verification | medium | done |
| A3 | **closed by measurement** — `52eec09`, 13–16 ns per framing; see A3 | low | done |
| A4 | **resolved procedurally** — every oracle citation carries observed / ceiling / headroom; no task | low | done |
| A5 | **fixed** — hard rule 3 names the exception and requires an ADR plus a measurement for any new hot-path lock | low | done |
| A6 | **fixed** — the hook cites the rule by name now; two claims withdrawn | low | done |

A1 and A2 shared a dependency — both wanted something in `fah-common`, the retry
policy and the log throttle — so they landed as one change, keeping the L1
surface to one review.

**PASS** — A1 and A2 were medium, A5 and A6 low, and all four are fixed. A3 is
low and was closed by measurement the same day (`52eec09`). A4 was low and is
resolved procedurally, not as work. Nothing blocks and nothing is owed.
