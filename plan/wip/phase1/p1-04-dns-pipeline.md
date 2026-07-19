# P1-04 — DNS Listeners and Pipeline

**Phase:** 1 · **Depends on:** p1-02 · **Model:** Sonnet

## Goal

`fah-dns` accepts queries on UDP/53 + TCP/53 and runs the pipeline front half:
verdict first, blocked-response synthesis.

## Context

ARCHITECTURE.md §DNS Pipeline + ADR-0001 (rules before cache). Wire types come
from `hickory-proto`; listeners bind per CONFIGURATION.md `[dns.listen]`.

## Scope

- UDP listener with EDNS(0) (payload sizes, OPT record echo), TCP listener
  (truncation fallback per RFC; length-prefixed framing).
- Pipeline: decode → Rule Engine verdict → Block ⇒ synthesize `0.0.0.0`/`::`
  (A/AAAA, TTL 10s per config), `$dnsrewrite` payloads honored; Allow/Pass ⇒
  continue to cache/upstream (stubbed until p1-05/06 — temporary passthrough
  forwarder acceptable behind a feature-gate or internal trait).
- Per-worker execution on the Tokio runtime — no central dispatcher, no locks.
- `QueryEvent` emission into a bounded channel (drop-on-full, counter).
- Tests: real DNS packets via `hickory-client` against an ephemeral port —
  blocked domain returns 0.0.0.0/:: with TTL 10, TC-flag path exercises TCP.

## Acceptance criteria

- Blocked queries never touch the network (assert no upstream call).
- Malformed packets dropped without panic (fuzz a corpus of truncated/garbage
  packets).
- Gates green.

## Out of scope

Cache (p1-05), real upstreams (p1-06), DoT/DoH listeners (Phase 3).

## Suggested prompt

> Read ARCHITECTURE.md §DNS Pipeline, ADR-0001, CONFIGURATION.md §[dns.*], and
> plan/wip/phase1/p1-04-dns-pipeline.md. Implement listeners + verdict-first
> pipeline with hickory-proto, QueryEvent emission, and packet-level tests.

## Completion note

**Design:** `fah-dns` (`crates/fah-dns/src/`) built directly on `hickory-proto`
wire types — no `hickory-server` (not in the offline registry cache, and the
crate's `Message::from_vec`/`to_vec` plus `op`/`rr` types are enough to hand-roll
listeners against ADR-0001's ordering).

- `qtype.rs` — the seam between wire `RecordType` and `fah_model::QueryType`
  (`fah_rules::Matcher::lookup`'s own type).
- `response.rs` — builds the blocked answer (null-IP for `A`/`AAAA` per
  CONTEXT.md; empty `NOERROR` for any other blocked query type — a
  conservative choice, not `NXDOMAIN`, mirroring the matcher's own
  unknown-type stance), error responses, and UDP truncation
  (`Message::truncate()` + re-encode when a reply exceeds the request's EDNS
  payload size or the 512-byte no-EDNS default).
- `rewrite.rs` — interprets `$dnsrewrite` (`fah_rules::DomainRule::dns_rewrite`
  is parsed-but-uninterpreted by design; `Matcher::rewrite`'s doc comment
  names `fah-dns` as the consumer). Supports AdGuard's bare-IP, bare-RCODE,
  and `RCODE;TYPE;VALUE` forms for `A`/`AAAA` — the forms the existing parser
  test corpus exercises; anything else (CNAME targets, TXT payloads)
  falls back to standard block synthesis, never a panic.
- `forwarder.rs` — `Forwarder` trait (native `async fn` in trait, no
  `async-trait` dep) is the Allow/Pass seam p1-06 replaces. `UdpForwarder` is
  the scope-permitted temporary passthrough: sequential ordered fallback over
  `[[dns.upstreams.servers]]`'s plain-UDP entries. Known gaps left for p1-06:
  no DoT/DoH, no upstream-truncation TCP retry, upstream query ID left as the
  client's own (randomizing it is a cache-poisoning hardening step that
  matters once p1-05 adds a cache to poison).
- `pipeline.rs` — `Pipeline<F: Forwarder>::handle`: decode → Rule Engine
  verdict (ADR-0001: before anything else) → `Block` synthesizes locally
  (forwarder never called — asserted by tests), `Allow`/`Pass` call the
  forwarder. Emits one `QueryEvent` per handled query into a bounded
  `mpsc::Sender` via `try_send`; a full channel drops and increments an
  `AtomicU64` counter (`Pipeline::dropped_events`, for p1-08) rather than
  back-pressuring the pipeline (ARCHITECTURE.md §Runtime Model). Generic over
  `Forwarder`, not a trait object — no dynamic dispatch on the hot path.
- `udp.rs`/`tcp.rs`/`server.rs` — one receive loop each, spawning a task per
  datagram/connection (no central dispatcher, ARCHITECTURE.md §Runtime
  Model). `Server::bind` binds both sockets to `[dns.listen]`'s address and
  exposes the actual bound addresses (needed because port `0` — tests only —
  gives UDP and TCP independent ephemeral ports).

**Tests (32 new, all green):** 27 unit tests across the modules above
(malformed-packet fuzz corpus, blocked-never-forwards, UDP truncation vs. TCP
no-truncation, dnsrewrite forms, EDNS DO-bit echo without `AD`, dropped-event
counting) plus 5 real-socket integration tests in
`crates/fah-dns/tests/server_integration.rs` — genuine UDP/TCP wire traffic
against a `Server` bound to an ephemeral port (`hickory-client` isn't in the
offline cache, so these hand-encode/decode with `hickory-proto` directly,
same wire format).

**Gates:** fmt/clippy (`-D warnings`)/test all green on the full workspace;
`fastadhunter/tests/layering.rs` passes with the new `fah-dns → fah-rules,
fah-model, fah-config` edges (a valid downward L3→L2/L1 dependency).

**Deferred, in scope elsewhere:** cache (p1-05, so `cache_hit` in every
`QueryEvent` this phase is `false`), DoT/DoH/parallel-fallback upstreams
(p1-06, `UdpForwarder` is explicitly temporary), metrics wiring for
`dropped_events` (p1-08), binary wiring of `Server`/`Pipeline`/channels
(later phase step, not this task's scope — mirrors p1-03's `ListManager`
being constructed but not yet wired into `fastadhunter`'s `main.rs`).

**Post-completion review:** chief-architect pass same day —
[docs/code-review/p1-04-review.md](../../../docs/code-review/p1-04-review.md).
Six findings fixed: upstream replies now validated (ID + response-type check,
mismatches discarded until timeout), forwarder receive buffer raised to
65535, TCP connections gained a 10 s idle timeout and RFC 7766 reuse
(multiple queries per connection), sub-512 EDNS payloads clamped per RFC
6891 §6.2.3, the matcher handle released before the upstream await, and the
test tempdir leak plugged. Tests now 29 unit + 6 integration. Where this
note conflicts with the review's "Fixes applied" section, the review is
current.
