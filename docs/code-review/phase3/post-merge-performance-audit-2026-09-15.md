# Audit — hot-path performance, memory and Rust quality at `e8e7cf8`

Read-only audit of `main` at `e8e7cf8`, 2026-09-15, working tree clean. Nothing
was changed; the allocation oracles were run as evidence. Scope is the hot-path
entry points only, production code — every line count below excludes the
`#[cfg(test)]` tail of its file.

## Summary

- No diff to review: the audit targets shipped code, not a changeset. The
  numbers are the current state, not a delta, and no regression claim is made.
- Hot path holds: 0 locks, 0 panics, 0 `unsafe`, 0 regex, 0 std guards across
  `.await` in the nine entry-point files.
- All five allocation oracles pass. Every per-query and per-request allocation
  found is transient — freed with the query or the request, not retained.
- Memory delta is 0 bytes: no new state, no new statics.
- Four findings, none blocking. One is a real log-flood path (F1); one is
  doc-versus-code drift on hard rule 3 (F2); one is a per-reply realloc on
  TCP/DoT (F3); one is hard rule 7 not describing the tree (F4).

## Scope

| File | Entry point |
| ---- | ----------- |
| `crates/fah-dns/src/pipeline.rs` | DNS query path — `Pipeline::handle` |
| `crates/fah-dns/src/udp.rs`, `tcp.rs` | DNS query path — accept loops |
| `crates/fah-dns/src/dot.rs` | DoT path |
| `crates/fah-http/src/proxy.rs` | HTTP request path — `serve_connection`, `judge`, `emit` |
| `crates/fah-http/src/sni.rs`, `tls_server.rs`, `intercept.rs` | HTTPS/SNI path |
| `crates/fah-dns/src/cache.rs`, `swr.rs`, `response.rs`, `qtype.rs` | per-query callees of `handle` |
| `fah-certs/leaf.rs`, `fah-stats/client_registry.rs`, `fah-api/password.rs` | not hot — key-space bounds only |

## Decisions

- Sharded `Mutex` in the DNS cache stays; the deviation from hard rule 3 is
  documented in [ARCHITECTURE.md](../../../ARCHITECTURE.md) §Runtime Model and
  in the `cache.rs` header. The rule text is what needs the exception, not the
  code.
- `judge` building `ModelRequest` unconditionally is not a finding: `events` is
  `Some` on every production wiring path (`main.rs:588`, `main.rs:1127`), so
  the `host`/`path`/`method` strings always have a consumer.
- `tokio::sync::Mutex` in `intercept.rs` is correct as written — the h2 arm
  clones the sender and drops the guard before awaiting, so multiplexing is not
  serialized; the H1 arm holds it deliberately because H1 is serial.

## Bugs found

### F1 — `warn!` per datagram on UDP reply failure, no rate limit

`crates/fah-dns/src/udp.rs:177`

```rust
warn!(error = %err, client = %client, "failed to send UDP DNS reply");
```

The `recv` path in the same file is backoff-limited by `RetryPolicy`
(`udp.rs:141`), and TCP routes client disconnects to `debug!`
(`tcp.rs:127`). This path does neither: a client network that stops accepting
replies produces one `warn` line per query. The RouterOS log buffer is small —
`pipeline.rs:236` already reasons about exactly this cost for the cleanup
sweep. Fix: classify with `is_client_disconnect` and add a counter, the shape
already in `tcp.rs:122-131`.

Severity: medium. Unauthenticated remote can sustain it; no memory growth, log
history loss only.

### F2 — hard rule 3 forbids locks on the hot path; the cache takes one per query

`crates/fah-dns/src/cache.rs:46`, `cache.rs:519`

Design, not defect — shard-selected `std::sync::Mutex`, argued in the file
header. The rule as written is contradicted by the shipped design. Fix: write
the exception into hard rule 3 in [CLAUDE.md](../../../CLAUDE.md).

Severity: low. Documentation only.

### F3 — one extra allocation per TCP/DoT reply

`crates/fah-dns/src/tcp.rs:181`

```rust
reply.splice(0..0, len);
```

`encode` returns `message.to_vec()` (`response.rs:164`) — an exactly-sized
`Vec` with no spare capacity, so prepending two bytes forces a grow plus a
memmove. Engineering principle 3. Fix: encode into a buffer with two reserved
leading bytes and write the prefix in place. TCP/DoT only, not UDP.

Severity: low. Small cost, small fix.

### F4 — hard rule 7 does not describe the tree

`CLAUDE.md` forbids Rust comments; `crates/` holds 8369 comment lines.
`.claude/hooks/no-rust-comments.sh` blocks agent edits only and cites "hard
rule 20" while CLAUDE.md numbers it 7. The rule, the hook and the code state
three different things.

Severity: low. Documentation only.

## Measurements

### Allocation oracles — run at `e8e7cf8`

| Oracle | Result | Ceiling |
| ------ | ------ | ------- |
| `fah-dns forward_alloc` | 3 passed | 10 / 16 / 19 allocations per query (hit inline name, hit heap name, block heap name); 17 / 24 per miss; jitter allowance 4 |
| `fah-http proxy_alloc` | 1 passed | 20 / 32 / 51 / 53 per request |
| `fah-http intercept_alloc` | 1 passed | 25 / 38 / 50 per request |
| `fah-rules url_lookup_alloc` | 1 passed | 0 allocations, whatever it decides |
| `fah-rules dedup_alloc_bound` | 1 passed | transient bounded under a 64 MiB hostile list |

### Hot-path scan — production code only

| Category | Items inspected | Problematic |
| -------- | --------------- | ----------- |
| allocations | 27 | 0 — 11 per-query/request and transient, 16 once per process |
| locks | 0 in entry-point files | 0 — the only per-query locks are cache shards (F2) |
| syscalls / IO / clocks | 15 | 0 — one `Instant::now` per query (`pipeline.rs:334`); dot.rs clocks are `#[cfg(feature = "diag-timing")]` |
| regex / fmt | 1 | 0 — no `Regex`; one `write!` into a preallocated buffer (`qtype.rs:28`) |
| error-path log sites | 41 | 1 — 40 are `debug!`, one is F1 |
| `unwrap` / `expect` / `panic!` / `unsafe` | 0 | 0 |
| std guard across `.await` | 0 | 0 — `cache.rs` has no `async fn` in production |
| retained container fields | 22 | 0 — all `Arc<…>` to bounded shared state |

### Transient versus retained

Every per-query and per-request allocation found is transient. None is memory
retention.

| Site | Lifetime | Why it exists |
| ---- | -------- | ------------- |
| `udp.rs:157` `buf[..len].to_vec()` | per datagram | moved into the spawned task |
| `tcp.rs:164` `vec![0u8; len]` | per message | `len` already capped at `MAX_MESSAGE_LEN` (16 KiB) |
| `qtype.rs:26` `domain_of` | per query | one `with_capacity` alloc, lowercased in place |
| `proxy.rs:579-613` host / path / method | per request | consumed by `ModelRequest`, always has an event sink |
| `pipeline.rs:442` `key.clone()` | until the SWR worker takes it | only on a claimed stale refresh; handed to a bounded queue |

### Key-space bounds

| Structure | Bound |
| --------- | ----- |
| `fah-certs/leaf.rs:214` | capacity + LRU eviction |
| `fah-stats/client_registry.rs:120` | `while len >= capacity` + eviction |
| `fah-api/password.rs:109-113` | `max_tracked`, expired entries dropped first |
| `fah-dns/swr.rs:93` | `workers * QUEUE_DEPTH_PER_WORKER` — configuration, not traffic |
| `fah-http/intercept.rs:97` `hello` | `MAX_HELLO_BYTES` (16 KiB) × `max_connections` |

Each bound has a test that fails if it is removed:
`capacity_evicts_the_least_recently_seen_client`,
`the_cache_evicts_the_least_recently_used_host_at_capacity`, and the
`per_address.len() <= 128` assertion in `password.rs:505`.

### Memory delta

0 bytes. No new struct field, no new static. Below the <1 MB threshold by
construction.

## Files changed

None. Read-only audit.

## Remaining TODOs

| Finding | Action | Owner decision |
| ------- | ------ | -------------- |
| F1 | classify the UDP send error, counter + `debug!` for disconnects | open |
| F2 | write the shard-lock exception into hard rule 3 | open |
| F3 | reserve two leading bytes in `encode_for_transport` | open |
| F4 | reconcile hard rule 7, the hook's rule number, and the tree | open |

**PASS WITH DEFERRED FINDINGS** — F1 medium, F2–F4 low. Nothing blocks.
