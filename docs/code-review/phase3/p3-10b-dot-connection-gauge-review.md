# P3-10b — DoT Connection Gauge — Review

**Task:** [`plan/wip/phase3/p3-10b-dot-connection-gauge.md`](../../../plan/wip/phase3/p3-10b-dot-connection-gauge.md)
**Plan:** [`plan/wip/phase3/p3-10b-dot-connection-gauge-plan.md`](../../../plan/wip/phase3/p3-10b-dot-connection-gauge-plan.md)
**Implemented:** 2026-09-14 · **Base:** `bf4e419` · **Findings:** not yet reviewed

## Implementation Summary

A DoT connection is now counted in a gauge of its own, reported on
`GET /api/v1/telemetry` as `counters.dns_dot_connections` with `active`, `peak`
and `closed_oversize`. `dot.rs` no longer passes `None` where the TCP listener
passes its gauge; `handle_connection`'s gauge parameter lost its `Option`
because both callers now pass one. Built as planned (D1–D9), with one forced
deviation (§Decisions as built, row 6).

Gates green: `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets --message-format=short -- -D warnings`, `cargo test --all-features
--workspace` (61 test targets, 0 failed). No bench owed — no hot path touched.

## Decisions as built

| # | Decision | As planned? |
| - | -------- | ----------- |
| 1 | `DnsDotConnections` / `DotConnectionGauge` are aliases, not new types; one gauge type, two instances (D1) | yes |
| 2 | `Server` owns `dot_gauge`, built in `bind()` unconditionally, exposed by `dot_connections()` (D2) | yes |
| 3 | Counted at accept, before the spawn; a failed handshake is counted while it is open (D3) | yes |
| 4 | `handle_connection(.., gauge: &TcpConnectionGauge)` — no `Option`; both callers pass `&open` (D4) | yes |
| 5 | Model field, registry slot, JSON shape (D5, D6, D8) | yes |
| 6 | Telemetry poll (D7) — **deviation**: `spawn_telemetry_poll` would have taken 8 arguments, which `clippy::too_many_arguments` (7) rejects. The three DNS gauges are grouped in a `DnsGaugeSources` struct, mirroring the existing `ProxyCounterSources`. No new task, no new tick, `TELEMETRY_POLL` unchanged | no — see note |
| 7 | CONTEXT.md not touched (D9) | yes |

Guard-drop order in `dot.rs`: `drop(open)` is explicit and runs before `_slot`
is dropped at task end, so `active` falls before the semaphore permit is
released — the gauge can never transient-read 65 under a 64-slot ceiling.

## Tests

| § | Test | Where |
| - | ---- | ----- |
| 7.1.1 | `a_served_dot_connection_is_counted_and_released` | `crates/fah-dns/src/dot.rs` |
| 7.1.2 | `a_dot_frame_over_the_bound_counts_closed_oversize` | `crates/fah-dns/src/dot.rs` |
| 7.1.3 | `the_connection_bound_queues_the_next_client_until_a_slot_frees` — extended with both oracles (pending handshake kept alive + gauge reads 64 at accept; then refill without a 65th) | `crates/fah-dns/src/dot.rs` |
| 7.2 | `each_dns_stream_listener_counts_into_its_own_gauge` — DoT then plain TCP, both gauges checked after each | `crates/fah-dns/tests/server_integration.rs` |
| 7.3 | `dns_dot_connections_round_trip_as_one_value` — incl. the TCP slot staying default | `crates/fah-metrics/src/registry.rs` |
| 7.4 | telemetry payload fixture + three pinned assertions | `crates/fah-api/tests/api.rs` |
| 7.5 | `the_shipped_configuration_blocks_at_every_layer` — polls `/api/v1/telemetry` every 500 ms for up to 20 s until `dns_dot_connections.peak >= 1` | `crates/fastadhunter/tests/shipped_path_e2e.rs` |

§7.2 uses a CA-minting `CertStore` in a `tempfile::tempdir()` and a client that
trusts that CA, rather than a dangerous verifier — no new test dependency.

## Mutation results — applied, run, reverted

| # | Mutation | Predicted | Observed |
| - | -------- | --------- | -------- |
| 1 | fresh `Arc::new(DotConnectionGauge::default())` shadows the passed gauge inside `run_with` (one edit reaching both the unit harness and the `Server` path) | §7.1.1, §7.2 fail | **fails** — §7.1.1, §7.1.2, §7.1.3 (`-p fah-dns --lib`) and §7.2 (`--test server_integration`) |
| 2 | `let open = Arc::clone(&gauge);` in place of `OpenConnection::enter(&gauge)` — keeps `closed_oversize`, kills `active`/`peak` | §7.1.1, §7.1.3 fail | **fails** — §7.1.1, §7.1.2, §7.1.3 |
| 3 | `main.rs` feeds `dns.tcp.snapshot()` to `set_dns_dot_connections` | §7.5 fails | **survives** — see below |
| 4 | `server.rs` `dot_connections()` returns `tcp_gauge` | §7.2 fails | **fails** — §7.2 |

### Mutation 3 survives §7.5 — limitation, not a code defect

`cargo test --all-features -p fastadhunter --test shipped_path_e2e` passed with
the mutation applied (2 passed, 13.09 s; 20.39 s unmutated).

Why: the e2e opens its two DoT connections and its two plain-TCP connections
sequentially, so both gauges read `peak == 1`. Feeding the TCP snapshot into the
DoT slot leaves the TCP slot correct and makes the DoT slot numerically
indistinguishable from the true DoT value. No assertion phrased over
`dns_tcp_connections` can detect it, because that group is untouched by the
mutation.

What §7.5 does catch, verified by mutations 1, 2 and 4 in their own targets: a
DoT gauge that is never incremented, never handed to the listener, or fetched
from the wrong field — the zeros-for-seven-days failure mode §7.5 exists for.

To close it, the e2e would have to make the two transports' figures differ — for
example open four concurrent DoT connections and poll for `peak >= 4`, which the
TCP gauge can never reach with two sequential connections. That is a test-design
change beyond the plan and was **not** made. Owner's call.

## Files changed

| File | Change |
| ---- | ------ |
| `crates/fah-model/src/engine.rs` | `DnsDotConnections` alias; `EngineCounters::dns_dot_connections` with `#[serde(default)]`, after the TCP field |
| `crates/fah-model/src/lib.rs` | re-export `DnsDotConnections` |
| `crates/fah-dns/src/tcp.rs` | `handle_connection` takes `&TcpConnectionGauge`; `if let Some(..)` dropped; caller passes `&open` |
| `crates/fah-dns/src/dot.rs` | `DotConnectionGauge` alias; `run`/`run_with`/`serve_connection` take the gauge; `OpenConnection::enter` in the accept loop; three tests |
| `crates/fah-dns/src/server.rs` | `dot_gauge` field, built in `bind()`, cloned into the DoT spawn, `dot_connections()` |
| `crates/fah-dns/src/lib.rs` | export `DotConnectionGauge` |
| `crates/fah-metrics/src/registry.rs` | slot, init, `set_dns_dot_connections`, `engine_telemetry()`, round-trip test |
| `crates/fastadhunter/src/main.rs` | `DnsGaugeSources`; poll writes the DoT snapshot; call site |
| `crates/fah-dns/tests/server_integration.rs` | cross-transport isolation test + three helpers |
| `crates/fah-api/tests/api.rs` | payload fixture + assertions |
| `crates/fastadhunter/tests/shipped_path_e2e.rs` | telemetry poll for the DoT figure |

## Other observations

- `crates/fah-http/tests/intercept_alloc.rs::warm_intercepted_requests_allocate_a_steady_amount`
  failed once on the first full-workspace run and passed standalone and on every
  later full run. It is an allocation-steadiness test unrelated to this change
  (no `fah-http` file was touched); load-sensitive under a parallel workspace run.

## Remaining TODOs

Documentation, each needing its own explicit yes (plan §8) — **none written**:

| Document | Edit |
| -------- | ---- |
| `API.md` | payload example `:227`, prose `:287-299` (the "handed no gauge" half is now false), poll note `:391` — required in the same change |
| `docs/project-state.md` | §Risk inventory close-out: F1 reads `dns_tcp_connections` alone; DoT has its own figure |
| `plan/wip/phase3/p3-10-post-merge-performance.md` | B2 names `counters.dns_dot_connections.peak` |
| `plan/wip/phase3/CLAUDE.md` | row 10b status |

No commit, no push, no tag. No task or phase moved.

## Findings

Reviewed 2026-09-14 against `bf4e419` plus the working tree, the task file and
the plan. Verification done here rather than taken from the report:
`cargo fmt --all -- --check` (clean), `cargo clippy --workspace --all-targets
-- -D warnings` (clean), `cargo test -p fah-dns --lib dot::` (9/9),
`--test server_integration` (13/13), `-p fah-metrics --lib` (24/24),
`--all-features -p fastadhunter --test shipped_path_e2e` (2/2, 23.5 s). The diff
was read in full; tests passing was not treated as evidence of correctness.

### F1 — should-fix — the e2e cannot tell the DoT slot from the TCP slot

`crates/fastadhunter/tests/shipped_path_e2e.rs:222-256`.

The DoT assertion is `counters.dns_dot_connections.peak >= 1` and the TCP one is
`(1..=2).contains(&dns_tcp_peak)`. Every DoT and TCP exchange in that test is
sequential, so both gauges settle at `peak == 1`. Feeding the TCP snapshot into
`set_dns_dot_connections` therefore satisfies both assertions — reported by the
implementer, who ran the mutation and recorded its survival rather than claiming
a pass, and re-confirmed here by reading the assertions against the test's
connection sequence.

Why it matters: §7.5 exists for one reason — it is the only test covering
`main.rs`'s poll wiring, the line whose failure mode is a soak reporting zeros,
or the wrong transport's figure, for seven days. The plan's acceptance row "each
test fails when its reporting is removed" is therefore only partly met. The gap
is in the oracle, not in the implementation: the production wiring is correct,
as mutations 1, 2 and 4 show in their own targets.

Smallest remediation: make the two transports' figures differ and assert
equality instead of a floor. Open three DoT connections concurrently, hold them
while polling, and assert `dns_dot_connections.peak == 3` together with
`dns_tcp_connections.peak <= 2`. The TCP gauge cannot reach 3 in that test, so
the swapped-slot mutation dies. `resolve_dot` opens and closes in one call, so
this needs a small helper that keeps N DoT streams open — the shape
`a_served_dot_connection_is_counted_and_released` already uses in `dot.rs`.

### F2 — should-fix — nothing pins D3's "counted at accept"

`crates/fah-dns/src/dot.rs:99` (the `OpenConnection::enter` site), with the
gauge assertions at `:582-651`.

D3 decided deliberately that a connection is counted **at accept**, before the
TLS handshake, so that a slow or hostile handshake — which holds a semaphore
slot — is visible in the figure that sizes `DOT_MAX_CONNECTIONS`. Every gauge
assertion in the suite is made over connections whose handshake **completed**:
`a_served_dot_connection_is_counted_and_released` exchanges a query on each of
its three, and the ceiling test fills all 64 with completed handshakes. Moving
`enter` out of the accept loop into `serve_connection` after
`start.into_stream(...)` would leave the whole suite green while silently
changing the meaning of the number p3-10's B2 row reads.

Smallest remediation: assert the gauge in a test where no handshake ever
completes. `plaintext_dns_on_the_dot_port_gets_no_answer` (`:537`) connects with
a bare `TcpStream` and speaks plain DNS at a TLS port; `active == 1` while that
connection is open and `0` once it is closed pins D3 with no new fixture.

### F3 — should-fix — API.md still says this counter does not exist

`API.md:227`, `:287-299`, `:391` are unchanged, and the code now publishes
`counters.dns_dot_connections`. The prose says the DoT listener "is handed no
gauge", that "a DoT connection appears in no `active`/`peak`", and that the
64-connection cap "has no counter and no config key" — the first two are now
false and the third is half false.

Not a code defect, and already tracked under §Remaining TODOs. It is recorded as
a finding because the repo rule is that a change contradicting a document
updates it in the same change, which makes the API.md edit a gate on the commit
rather than a follow-up. The plan's §8 lists the three sites; the owner's yes is
still owed.

### F4 — note — the D7 deviation is sound, and the plan was wrong, not the code

`crates/fastadhunter/src/main.rs:1106-1112`, `:714-718`, `:1125-1137`.

`spawn_telemetry_poll` already took seven arguments; the plan's "add `dns_dot`
after `dns_tcp`" would have made eight, which `clippy::too_many_arguments`
rejects under `-D warnings`. Grouping the three DNS gauges into
`DnsGaugeSources` mirrors `ProxyCounterSources` a few lines above, keeps the
struct private to the binary, allocates nothing per tick and changes no
behaviour. Checked and found acceptable; D7 is what was inaccurate, because the
plan did not count the parameters already there.

Process note, not a code finding: the handover asked the implementer to stop and
report rather than pick a route when the plan could not be implemented as
written. It implemented and then reported — in this file and in the handoff line,
with the constraint named. Disclosure was complete and the substance is the
minimal in-idiom fix; the owner accepted it on 2026-09-14.

### F5 — note — the aliases are transparent, by decision

`crates/fah-model/src/engine.rs:105`, `crates/fah-dns/src/dot.rs:24`.

`DnsDotConnections` and `DotConnectionGauge` are type aliases, so the compiler
cannot stop a TCP gauge being passed where a DoT one is expected, or a TCP
snapshot being stored in the DoT slot. That is D1's accepted trade-off, taken
against roughly 25 lines of duplicated struct. Of the two places the mistake can
be made, one is covered — mutation 4, `dot_connections()` returning `tcp_gauge`,
dies in §7.2 — and the other is F1. Recorded so a later reviewer does not
re-open a settled decision, and so F1 is read as the one place where the missing
type safety actually bites.

### F6 — note — three test helpers duplicate helpers that already existed

`crates/fah-dns/tests/server_integration.rs:620-660`.

`stream_roundtrip` is `tcp_roundtrip` (`:198-211`) with the connect lifted out —
identical framing, reads and buffer sizing. `await_connections` is
`await_inflight` (`:483-500`) with one type changed. A third copy of the same
polling loop is `await_gauge` in `dot.rs:381-396`; it lives in a different test
target and cannot share code without a new fixture, so it is not counted here.

Remediation, worth doing only while the file is already open: have
`tcp_roundtrip` connect and delegate to `stream_roundtrip`, and express the two
waiters as one helper over a snapshot closure. No behaviour change either way.

### Categories checked with nothing found

- **Plan compliance.** D1, D2, D3, D4, D5, D6, D8 and D9 are built as written,
  site for site: the aliases; the unconditional `dot_gauge` in `bind()` with
  `dot_connections()` beside `tcp_connections()`; `enter` in the accept loop
  before the spawn; the `Option` gone from `handle_connection` with both callers
  passing `&open`; the model field with `#[serde(default)]` in the documented
  position; the registry's four sites; CONTEXT.md untouched. D7 is F4.
  Out-of-scope boundaries held: `DOT_MAX_CONNECTIONS` unchanged, no default
  changed, TCP gauge semantics identical, no DoH gauge, no dashboard change, no
  rename.
- **Correctness, lifecycle, cancellation.** `drop(open)` is explicit and runs
  before `_slot` falls out of scope, so the decrement always precedes the
  semaphore release and the gauge cannot transiently read 65 under a 64-slot
  ceiling. A task aborted by `Server::shutdown`, or one that panics, drops the
  guard while unwinding and decrements. No `continue` in the accept loop can
  strand a count, because the guard is created after the last one. No new
  `unwrap`, `expect` or panic path in production code.
- **Concurrency.** `active` and `peak` are maintained by the same RAII guard TCP
  uses; every counter is a `Relaxed` atomic written by connection tasks and read
  only by the 10 s poll. `snapshot()` reads three atomics separately, so a triple
  can straddle a change — inherited from the TCP gauge, clamped by
  `peak().max(active)`, and named in the plan's §6 rather than introduced here.
  No lock is taken and nothing is held across an `await`.
- **Architecture.** Layering unchanged: `fah-dns` reaches only `fah-model` and
  `fah-common`; the binary stays the only crate that knows both `fah-dns` and
  `fah-metrics`. No new dependency and no new public type beyond the two aliases,
  plus the binary-private `DnsGaugeSources`. Nothing reaches past the stated
  problem.
- **Performance.** The DNS hot path is untouched — no per-query work was added.
  Per message, `handle_connection` lost an `Option` check. Per DoT connection the
  cost is one `fetch_add`, one `fetch_max`, one `fetch_sub` and one `Arc` clone,
  identical to what TCP has always paid, on a path already performing a TLS
  handshake. No formatting, logging, syscall or serialization was added to a
  repeating path. No bench is owed and none was run, consistent with the plan.
- **Memory.** One extra gauge per process (two `AtomicU32` and one `AtomicU64`
  behind an `Arc`), one extra `ArcSwap` slot in the registry, three more `u64` in
  the telemetry payload. Nothing grows with traffic or uptime. The single
  allocation per 10 s tick — a fresh `Arc` stored into the `ArcSwap` — is the
  pattern the TCP and UDP groups already use, not new pressure.
- **Rust quality.** No unnecessary clone or copy: the gauge travels as `&` into
  `serve_connection` and `handle_connection`, and as one `Arc` clone per
  connection. `Send`/`Sync` come from `Arc` plus atomics. Error propagation in
  the touched paths is unchanged.
- **Regression.** `handle_connection` is `pub(crate)` with exactly two callers,
  both updated; no external consumer exists. TCP behaviour is unchanged.
  `#[serde(default)]` keeps an older payload deserializable, which matters at
  `crates/fastadhunter/tests/http_e2e.rs:507`, the one place `EngineCounters` is
  deserialized rather than indexed. The unrelated
  `intercept_alloc::warm_intercepted_requests_allocate_a_steady_amount` flake the
  implementer recorded touches no file in this change and is load-sensitive under
  a parallel workspace run; it is not attributable here.

### Status

**PASS WITH DEFERRED FINDINGS.**

The implementation is correct, in scope and faithful to the plan; nothing found
here requires a change to production code. F1 and F2 are oracle gaps — two
mistakes the tests were meant to catch do not make them fail — and F3 is the
document edit the repo rule requires in this same change.

Whether F1 and F2 are closed now or deferred is the owner's decision, not this
review's. Deferring them leaves `main.rs`'s slot wiring and D3's "counted at
accept" semantics covered by inspection alone, going into a seven-day soak that
reads exactly those two things.
