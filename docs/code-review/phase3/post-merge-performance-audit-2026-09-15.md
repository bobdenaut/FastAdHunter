# Audit — hot path, memory and Rust quality at `2b03a30`

Replaces the first pass of the same date (commit `2b03a30`), whose lock, panic
and oracle figures were wrong; the corrected counts are below and the reason the
first pass undercounted is A7.

## Summary

- `base commit: e8e7cf8` · `head commit: 2b03a30` · `mode: SNAPSHOT`. The working
  tree is clean; `git diff --stat e8e7cf8..HEAD` touches one documentation file
  and no code, so there is no changeset to review.
- SNAPSHOT consequences: new state, new error paths, new `.await`s,
  `Send`/`Sync` changes, memory delta and before/after oracle numbers are
  **N/A — SNAPSHOT** and are not reported as zeros.
- Scope is the four hot-path entry points plus their one-level callees, 13
  files. Every count is production-only; the `#[cfg(test)]` tail is excluded.
- Seven findings, none blocking. Two medium: an accept loop with no backoff
  (A1), and four unrated `warn!` paths that are one defect in four places and
  share one fix (A2). Then one per-reply realloc (A3), two method findings
  (A4, A7) and two documentation findings (A5, A6).
- All five oracles pass. Observed counts are reported with their ceilings and
  headroom: 8 of 12 ceiling checks clear by exactly the 4-allocation jitter
  allowance, so the ceilings equal today's measurements.
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
  header; A5 asks for the rule text, not a code change); `tokio::sync::Mutex` at
  `intercept.rs:486` and `swr.rs:173`, both scoping the guard so the `.await`
  that matters runs outside it; `judge` building `ModelRequest` unconditionally,
  since `events` is `Some` on every production wiring path (`main.rs:588`,
  `main.rs:1127`).
- The oracles run under `cargo test`, i.e. the dev profile: `debug_assert!` is
  live and nothing is optimized. The numbers are regression detectors on this
  dev box, not production allocation counts for the RB5009.
- No bench was run and no regression claim is made. With no code diff there is
  nothing for a bench to discriminate.

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
end. Smallest fix: promote `RetryPolicy` (`fah-dns/src/backoff.rs:13`, currently
`pub(crate)`) to `fah-common`, then use it in `accept_loop`. Hard rule 1 forbids
`fah-http` importing `fah-dns`, so copying the policy is not an option and the
L1 move is the only non-duplicating fix.

Severity: medium. No memory growth; CPU starvation of the DNS listeners on the
same runtime is the real cost.

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

**The shared fix is the throttle, not the classification.** A counter plus
"log the first occurrence and every Nth, with the total in the fields" belongs
in `fah-common` (L1, reachable from both `fah-dns` and `fah-http`) — engineering
principle 4. Four hand-rolled counters would be the same logic copied four
times.

**Classification stays per site**, because the error kinds differ:

- `udp.rs:176` — `is_client_disconnect` does **not** transfer here. It matches
  `BrokenPipe`, `ConnectionReset`, `ConnectionAborted` and `UnexpectedEof`,
  which are stream errors UDP does not produce. `send_to` returns
  `ConnectionRefused` (ICMP port unreachable), `HostUnreachable`,
  `NetworkUnreachable` and `PermissionDenied` (a firewall reject); those belong
  at `debug!`. `MessageSize` stays at `warn!` — that one is our own truncation
  bug. All four `ErrorKind`s are stable on MSRV 1.96.
- `response.rs:165` — an encode failure is always ours; no demotion, throttle
  only.
- `dot.rs:194` — a `JoinError` is either a panic or a cancellation; the panic
  case is ours and stays at `warn!`.
- `tcp.rs:129` — classification already correct; throttle only.

Severity: medium, carried by `udp.rs:176`, which an unauthenticated remote can
sustain. The other three are low on their own. No memory growth anywhere; log
history loss only.

### A3 — one extra allocation per TCP/DoT reply, and the obvious fix is invalid

`crates/fah-dns/src/tcp.rs:183`

```rust
    reply.splice(0..0, len);
```

`encode` returns `message.to_vec()` (`response.rs:164`) — an exactly-sized `Vec`
with no spare capacity, so prepending the two length bytes forces a grow plus a
memmove. Engineering principle 3. TCP/DoT only, not UDP.

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

Fix: hold until a measurement justifies one of the three. Engineering principle
8 — the gain is one realloc per reply on the minority transports.

Severity: low. Small cost, no correct small fix.

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

Fix: report observed, ceiling and headroom together, as the oracle table below
does. No code change.

Severity: low. Method, not code.

### A5 — hard rule 3 forbids locks on the hot path; the cache takes one per query

`crates/fah-dns/src/cache.rs:46`, `cache.rs:519`

Design, not defect — a shard-selected `std::sync::Mutex`, argued in the file
header and in [ARCHITECTURE.md](../../../ARCHITECTURE.md) §Runtime Model. The
rule as written is contradicted by the shipped design.

Fix: write the exception into hard rule 3 in [CLAUDE.md](../../../CLAUDE.md).

Severity: low. Documentation only.

### A6 — hard rule 7 does not describe the tree, and the hook cites a different number

`CLAUDE.md` hard rule 7 forbids Rust comments. `crates/**/*.rs` holds 8369
comment lines (6992 of them under `crates/*/src`).
`.claude/hooks/no-rust-comments.sh:3` blocks agent edits only, and cites "hard
rule 20" while `CLAUDE.md` numbers it 7. The rule, the hook and the code state
three different things.

Fix: the hook's number is a one-line change. What the rule should say — a total
ban, or `//` banned and `///` allowed — is an owner decision, not an audit
finding.

Severity: low. Documentation only.

### A7 — cutting a file at the first `#[cfg(test)]` undercounts `cache.rs`

`crates/fah-dns/src/cache.rs:675`, `:685`, `:862`

The audit recipe cuts each file at the first `#[cfg(test)]`. `cache.rs` carries
that attribute on two individual methods (`len`, `queue_len`) before the test
module at `:862`, so the cut drops 187 lines of production code —
`note_lookup`, `clean`, `cleanup_stats`, `positive_ttl`, `negative_ttl` — and
with them 2 lock sites and 2 clock reads. That is how the first pass of this
audit reached "0 locks".

Fix: anchor the pattern at column 0 — `grep -n '^#\[cfg(test)\]'`. Of the 13
files in scope only `cache.rs` differs between the two patterns.

Severity: low, but it invalidated part of this audit's first pass.

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
| `tcp.rs:183` | `reply.splice(0..0, len)` | per TCP/DoT reply | realloc + memmove — **A3** |
| `response.rs:70-129` | 10 × query / record / name clone | per synthesized reply | hickory owns its records |
| `response.rs:164` | `message.to_vec()` | per reply | exact-size wire buffer |
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
once-per-process. One finding (A6); nothing retained.

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

None. Read-only audit.

## Remaining TODOs

| Finding | Action | Severity | Owner decision |
| ------- | ------ | -------- | -------------- |
| A1 | move `RetryPolicy` to `fah-common`, use it in `server.rs accept_loop` | medium | open |
| A2 | one throttle helper in `fah-common`, then wire the four sites; classify the UDP send error at `udp.rs:176` — `ConnectionRefused`, `HostUnreachable`, `NetworkUnreachable`, `PermissionDenied` → `debug!` | medium | open |
| A3 | hold; the offset encode is invalid, and the three valid fixes each need a measurement first | low | open |
| A4 | report observed / ceiling / headroom whenever an oracle is cited | low | open |
| A5 | write the shard-lock exception into hard rule 3 | low | open |
| A6 | fix the hook's rule number; decide what hard rule 7 should say | low | open |
| A7 | anchor the `#[cfg(test)]` cut at column 0 in the audit recipe | low | open |

A1 and A2 share a dependency: both want something in `fah-common` — the retry
policy and the log throttle. Doing them in one change keeps the L1 surface to
one review.

**PASS WITH DEFERRED FINDINGS** — A1 and A2 medium, A3–A7 low. Nothing blocks.
