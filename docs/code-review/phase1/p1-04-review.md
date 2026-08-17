# Code Review — p1-04 DNS Listeners and Pipeline

**Scope:** `crates/fah-dns/src/{lib.rs, pipeline.rs, response.rs, rewrite.rs,
qtype.rs, forwarder.rs, udp.rs, tcp.rs, server.rs}`,
`crates/fah-dns/tests/server_integration.rs`, `crates/fah-dns/Cargo.toml`,
workspace `Cargo.toml` ·
**Reviewer:** chief architect pass · **Date:** 2026-07-18 ·
**Status:** findings 1–6 fixed same day (see "Fixes applied" below), 7–9 are
notes with no code change required. Gates green.

## What was delivered (implementation report)

p1-04 done, marked DONE in the phase plan table, all gates green
(fmt/clippy/test workspace-wide, 32 new tests at delivery).

Built `fah-dns` (`crates/fah-dns/src/`) directly on `hickory-proto` wire
types — no `hickory-server` (not in the offline registry cache; `Message`
encode/decode is enough to hand-roll listeners):

- **UDP + TCP listeners** — EDNS(0) payload size honored, truncation via
  `Message::truncate()` on oversized UDP replies, RFC 1035 length-prefixed
  TCP framing. One task per datagram/connection, no central dispatcher
  (ARCHITECTURE.md §Runtime Model).
- **Pipeline** — decode → Rule Engine verdict (ADR-0001: before anything
  else) → `Block` synthesizes locally (null-IP for A/AAAA per CONTEXT.md,
  empty `NOERROR` for other blocked types — deliberately not `NXDOMAIN`);
  `Allow`/`Pass` go to a `Forwarder` trait seam. Generic over the forwarder,
  no dyn dispatch on the hot path.
- **`$dnsrewrite` interpretation** (`rewrite.rs`) — bare IP, bare RCODE,
  `RCODE;TYPE;VALUE` forms; anything else falls back to standard block
  synthesis, never a panic.
- **`UdpForwarder`** — scope-permitted temporary passthrough: sequential
  ordered fallback over plain-UDP upstreams. Gaps documented for p1-06:
  no DoT/DoH, no truncation TCP retry, no upstream query-ID randomization.
- **`QueryEvent` emission** — bounded `mpsc`, `try_send`, drop-on-full with
  an `AtomicU64` counter exposed for p1-08.
- **Tests** — 27 unit + 5 real-socket integration (hand-encoded wire format;
  `hickory-client` not offline-cached): blocked-never-touches-forwarder,
  malformed-packet fuzz corpus, UDP truncation vs TCP, dnsrewrite forms,
  EDNS DO-bit echo without `AD`.

## Overall assessment

The shape is right. Verdict-before-everything is structurally enforced (the
`Block` arm literally cannot reach the forwarder, and both a unit spy and a
real-socket integration test assert it); the `Forwarder` trait is the correct
p1-06 seam; `Transport` pushing the truncation budget into the pipeline keeps
decode single-pass; the drop-on-full event channel matches the runtime model;
the null-IP / empty-NOERROR / no-AD-bit choices are all defensible and
documented at the decision site. Verified against the vendored hickory-proto
0.26.1 source that `Message::truncate()` retains the question section and the
EDNS OPT record, so the TC-bit path is RFC-clean.

But the review turns up one finding with security weight (unvalidated
upstream replies) and a small cluster of robustness gaps concentrated in the
two places that talk to untrusted peers — the forwarder and the TCP listener.
Nine findings, six requiring fixes.

## Findings

### 1. MEDIUM-HIGH (security) — forwarder accepted any datagram as the upstream's answer

`forwarder.rs` `try_upstream`: the socket is `connect`ed (kernel filters by
source address/port), but the first datagram that arrived was decoded and
returned with **no check that its ID matches the query's or that it is a
response at all**. An off-path spoofer who guesses the ephemeral source port
wins outright — the 16-bit transaction ID, the standard second factor every
resolver checks, bought zero protection. A stale or duplicated reply from the
upstream itself would also have been accepted for the wrong query. The
pipeline then *overwrites* the reply's ID with the client's
(`forward_or_servfail`), masking the mismatch end to end.

**Fix:** loop on `recv` until the deadline, accepting only a decodable
message with `message_type == Response` and the matching ID; everything else
is discarded and the wait continues.

### 2. MEDIUM — 4096-byte receive buffer truncated large upstream replies

`try_upstream` received into `[0u8; 4096]`. The client's own EDNS payload
size (up to 65535) is forwarded verbatim upstream, so a compliant upstream
may answer with more than 4096 bytes; the kernel silently truncates the
datagram to the buffer, decode fails mid-record, the upstream is treated as
failed, and a perfectly good large answer becomes SERVFAIL after the fallback
list is exhausted. **Fix:** 65535-byte buffer (RFC 6891 ceiling).

### 3. MEDIUM — TCP connections could idle forever, and served only one query

`tcp.rs` `handle_connection` had no read timeout: a client that connects and
sends nothing (or half a length prefix) parked a task and a file descriptor
indefinitely — slowloris-shaped, and a direct violation of CLAUDE.md's
"bounded everything" on a 1 GB shared-RAM target. Separately, the connection
closed after one exchange; RFC 7766 §6.2.1 expects servers to support
multiple queries per connection, and real stub resolvers reuse connections
after a TC-bit retry. **Fix:** 10 s idle timeout on both the length prefix
and the body read, and a serve-until-EOF/timeout loop; malformed messages
still close the connection (RFC 7766 §6.2.4).

### 4. LOW — advertised EDNS payload below 512 wasn't clamped

`response::max_udp_payload` returned the raw advertised value. RFC 6891
§6.2.3: values below 512 MUST be treated as 512. A client advertising e.g.
100 forced needless truncation of every answer (the blocked A-response alone
is larger). `skeleton()` already clamped the *echoed* value — the budget path
just forgot the same clamp. **Fix:** clamp in `max_udp_payload`.

### 5. LOW — misleading comment: matcher *was* held across the upstream await

`pipeline.rs` claimed "nothing here holds it across a suspension point", but
the `Allow`/`Pass` arms awaited `forward_or_servfail` with the `Arc<Matcher>`
handle live. Harmless memory-safety-wise (it's an `Arc`, not a lock or an
`arc_swap` guard — verified `ListManager::matcher()` returns
`Arc<Matcher>` via `load_full`), but a slow upstream pinned a swapped-out
ruleset's memory for the forward's duration, and the comment asserted the
opposite of the code. **Fix:** restructure — verdict arms produce the
synthesized response or `None`, the handle is dropped, *then* the forward
awaits.

### 6. LOW (test hygiene) — `std::mem::forget(TempDir)` leaked directories on disk

Both the pipeline unit tests and the integration harness `forget` the
`TempDir` guard, so its destructor never runs and every test invocation
permanently deposits a directory in the OS temp dir. Days of TDD accumulate
real garbage. **Fix:** return the guard and hold it in each test
(`_data_dir`), restoring cleanup-on-drop.

### 7. NOTE (no change) — error responses don't echo the question section

`response::error` answers FORMERR/NOTIMP with an empty question section.
For FORMERR there is often no parseable question to echo; for NOTIMP echoing
would be friendlier but nothing requires it. Revisit only if a real client
misbehaves.

### 8. NOTE (no change) — `Server::shutdown` aborts listeners, not in-flight query tasks

Per-datagram/per-connection tasks spawned by the listeners hold their own
pipeline `Arc`s and finish naturally. Short-lived by construction (bounded by
forwarder timeout + TCP idle timeout after fix 3). Graceful drain belongs to
the binary-wiring step, not here.

### 9. NOTE (no change) — TCP reply length `unwrap_or(u16::MAX)` is unreachable

`encode_for_transport` with the TCP budget already caps the encoded reply at
65535 bytes, so the `u16::try_from(reply.len())` fallback can't fire; it's
defensive, not dead-wrong. Fine as is.

## Verdict

Sound architecture, correct ADR-0001 enforcement, genuinely good test
instincts (the spy-forwarder assertion and the hand-rolled wire tests are
exactly right for the offline environment). The findings cluster where
untrusted bytes enter: upstream replies taken on faith (1, 2) and TCP
clients allowed to squat (3). All fixable without touching the architecture.
Fix 1–6 before p1-05 builds the cache on top — finding 1 especially, since a
cache turns a one-shot spoofed reply into a poisoned entry served for its
TTL.

## Fixes applied (2026-07-18)

All six findings fixed the same day; 7–9 need no change.

1. **Upstream reply validation** (`forwarder.rs`) — `try_upstream` now loops
   on `recv` inside the timeout, accepting only a decodable message with the
   request's ID and `message_type == Response`; mismatched/garbage datagrams
   are discarded and the wait continues. New test
   `a_mismatched_id_reply_is_discarded_and_the_matching_one_accepted` sends a
   wrong-ID reply, then garbage, then the genuine answer.
2. **65535-byte receive buffer** (`forwarder.rs`) — `MAX_UDP_REPLY` const,
   heap-allocated per attempt (forward path already allocates; hot-path
   budget targets the matcher).
3. **TCP idle timeout + connection reuse** (`tcp.rs`) —
   `TCP_IDLE_TIMEOUT = 10s` on both the length-prefix and body reads;
   `handle_connection` now serves queries in a loop until EOF, timeout, or a
   malformed message (which still closes, RFC 7766 §6.2.4). New integration
   test `one_tcp_connection_serves_multiple_queries`.
4. **RFC 6891 clamp** (`response.rs`) — `max_udp_payload` clamps advertised
   values below 512 up to 512. New test
   `a_sub_512_edns_payload_is_clamped_up_per_rfc_6891`.
5. **Matcher released before the forward await** (`pipeline.rs`) — verdict
   arms now produce `(Verdict, Option<Message>)`; the `Arc<Matcher>` handle
   is dropped before `forward_or_servfail` runs, and the comment now states
   what actually happens.
6. **TempDir guards held, not forgotten** (`pipeline.rs` tests,
   `tests/server_integration.rs`) — helpers return the guard; every test
   holds it as `_data_dir`, so directories are removed on drop.

**Verification:** `cargo fmt --check` clean;
`cargo clippy --workspace --all-targets --offline -- -D warnings` clean;
`cargo test --workspace --offline` all green — fah-dns 29 unit (was 27)
+ 6 integration (was 5), fah-rules 70, workspace unchanged elsewhere.
