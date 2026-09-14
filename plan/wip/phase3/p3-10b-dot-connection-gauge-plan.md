# P3-10b — DoT Connections Get Their Own Gauge — Implementation Plan

**Task:** [`p3-10b-dot-connection-gauge.md`](p3-10b-dot-connection-gauge.md)
**Phase:** 3 · **Depends on:** p3-05 · **Source:** p3-10 Track A, A5 · **Model:** Fable

Every decision below is settled here. The implementer writes code, not design.
Where a route was rejected, the rejection and its reason are recorded so the
review does not re-open it.

## 1. Objective and scope

A DoT connection is counted in a gauge of its own, reported on
`GET /api/v1/telemetry` as `counters.dns_dot_connections` with `active`,
`peak` and `closed_oversize`, and no DoT connection is ever counted in
`counters.dns_tcp_connections`.

In scope: `fah-model` (one field, one alias), `fah-dns` (`dot.rs`, one `tcp.rs`
signature, `server.rs` ownership and accessor, `lib.rs` export), `fah-metrics`
(registry slot + setter), `crates/fastadhunter/src/main.rs` (telemetry poll),
tests in five places, API.md (separate approval).

Out of scope is listed in §10 and is binding.

## 2. Existing code paths — verified, not assumed

| Fact | Site |
| ---- | ---- |
| DoT hands `None` where TCP hands its gauge | `crates/fah-dns/src/dot.rs:152` |
| The gauge type: `ConnectionGauge` + `AtomicU64 closed_oversize`, `AsRef<ConnectionGauge>`, `snapshot()` | `crates/fah-dns/src/tcp.rs:26-47` |
| TCP counts a connection at accept, before the spawn, and drops the guard at the end | `crates/fah-dns/src/tcp.rs:111-118` |
| `handle_connection` uses its `gauge` argument for exactly one thing: `closed_oversize.fetch_add(1)` | `crates/fah-dns/src/tcp.rs:143`, `:154-157` |
| `handle_connection` has exactly two callers — `tcp.rs:114` (`Some`) and `dot.rs:152` (`None`) | repo-wide grep |
| `active`/`peak` come from the RAII guard, not from `handle_connection` | `crates/fah-common/src/connections.rs` (`OpenConnection::enter` / `Drop`) |
| `peak` is a process-lifetime high-water mark here: `snapshot()` reads `peak()`, never `take_peak()` | `crates/fah-dns/src/tcp.rs:33-41` |
| `Server` owns `tcp_gauge` and `udp_gauge`, created in `bind()`, exposed by `tcp_connections()` / `udp_inflight()` | `crates/fah-dns/src/server.rs:36-37`, `:95-96`, `:145-151` |
| The DoT listener task is spawned in `serve()` and takes no gauge | `crates/fah-dns/src/server.rs:125-133` |
| DoT slots: a semaphore permit is acquired **before** `accept()` and held for the whole connection | `crates/fah-dns/src/dot.rs:62-66`, `:91-97` |
| The registry stores each counter group as one `ArcSwap` value and copies it into `engine_telemetry()` | `crates/fah-metrics/src/registry.rs:66-67`, `:116-117`, `:241-247`, `:318-319` |
| The binary's 10 s poll is the only writer of both DNS gauges | `crates/fastadhunter/src/main.rs:1119-1130`, call site `:714` |
| API.md states in prose that DoT is **not** counted | `API.md:227`, `:287-299`, `:391` |
| `dot` is already Client Transport vocabulary | `CONTEXT.md:113-118` |
| There is no Prometheus exporter for these counters | grep: no `HELP` / `prometheus` in `fah-metrics`, `fah-api` |
| DoH is not in `fah-dns` at all — it is `/dns-query` on the API listener | `crates/fah-api/src/doh.rs`, `crates/fah-api/src/routes.rs:125` |

## 3. Design decisions

### D1 — One gauge type, two instances; two aliases for honest names

**Decided.** DoT reuses the existing `TcpConnectionGauge` type and the existing
`fah_model::DnsTcpConnections` shape, as a **second instance**. Two type
aliases keep the names honest at the two boundaries a reader meets:

```rust
pub type DnsDotConnections = DnsTcpConnections;
pub type DotConnectionGauge = crate::tcp::TcpConnectionGauge;
```

Why: DoT is DNS over TLS over TCP — the same framing loop, the same 16 KiB
bound, the same three figures. The transport identity belongs to the *instance*
and to the JSON field name, not to the type. This duplicates no logic and
renames nothing.

Rejected:

- **A new `DotConnectionGauge` struct plus a new `DnsDotConnections` struct.**
  About 25 duplicated lines whose only difference is the name, against
  principle 4. If DoT ever needs a fourth figure the alias becomes a struct
  then, not now.
- **Renaming `TcpConnectionGauge` to `DnsConnectionGauge` and
  `DnsTcpConnections` to `DnsConnectionCounts`.** Correct in the abstract, about
  21 mechanical edit sites across five crates, none of which this task otherwise
  needs to touch. Scope creep in a task whose deliverable is "count DoT", and it
  puts a rename in the same diff the soak's instrument has to be trusted from.
- **Sharing the TCP gauge instance.** Already rejected by the owner on
  2026-09-13: one number for two transports cannot answer a question about one
  of them.

### D2 — The gauge is owned by `Server`, created unconditionally

**Decided.** `Server` gains `dot_gauge: Arc<TcpConnectionGauge>`, built in
`bind()` next to `tcp_gauge` **whether or not DoT is enabled**, and exposed by
`pub fn dot_connections(&self) -> Arc<DotConnectionGauge>`.

Why: it mirrors `tcp_connections()` and `udp_inflight()` exactly, and it keeps
`Option` out of the telemetry poll. With DoT disabled the three figures stay
zero, which is what `dns_udp_inflight` already does when `udp_max_inflight = 0`
(API.md:300-305). Cost of the always-on gauge: two `AtomicU32` and one
`AtomicU64` per process.

Rejected: `Option<Arc<…>>` threaded through `serve()`, the accessor and
`spawn_telemetry_poll`, to save 16 bytes and gain a branch on every poll.

### D3 — A connection is counted at accept, not after the handshake

**Decided.** `OpenConnection::enter(&gauge)` runs in `run_with`, after
`accept()` and `set_nodelay`, before `tokio::spawn` — the exact position TCP
uses (`tcp.rs:111`). The guard moves into the spawned task and is dropped after
`serve_connection` returns, beside the existing `_slot`.

Why: the number has one job — to say whether `DOT_MAX_CONNECTIONS = 64` covers
this house (p3-10 B2). The semaphore permit is taken **before** `accept()` and
held across the TLS handshake, so a connection stuck in a handshake occupies a
slot. A gauge that counted only post-handshake connections would under-report
exactly the case that saturates the cap — a slow or hostile handshake — and
would answer a different question from the one the cap poses.

Consequence, stated so the review does not file it as a bug: a connection that
fails its ClientHello, fails the handshake or times out is counted as `active`
for as long as it is open, and can raise `peak`. That matches TCP, where a
client that connects and sends nothing is counted for its idle timeout.

### D4 — `closed_oversize` lands in the same gauge, and the `Option` goes away

**Decided.** Yes, `closed_oversize` is part of this gauge — a DoT frame over
`MAX_MESSAGE_LEN` is bounded today but invisible, and that is the half of A5
that has nothing to do with the cap.

Because both call sites now pass a gauge, the parameter stops being optional:

```rust
pub(crate) async fn handle_connection<S: AsyncRead + AsyncWrite + Unpin, F: Forwarder>(
    mut stream: S,
    pipeline: &Pipeline<F>,
    client_ip: std::net::IpAddr,
    transport: Transport,
    gauge: &TcpConnectionGauge,
) -> std::io::Result<()>
```

and the body loses its `if let Some(gauge)`. `tcp.rs:114` passes `&open`,
`dot.rs:152` passes `&open` — both by deref coercion from
`OpenConnection<TcpConnectionGauge>`.

Why: the `Option` existed for one reason, the caller this task is fixing
(principle 14 — delete what is no longer justified). It also removes a silent
failure mode: no future listener can reuse this loop and count nothing without
first being handed a gauge.

Note for §7.5: this makes the task file's literal mutation ("put `dot.rs` back
to passing `None`") **not compile**. The equivalent mutation is specified there
instead.

### D5 — Model: one field, `#[serde(default)]`, placed after the TCP one

**Decided.** In `crates/fah-model/src/engine.rs`, beside line 89:

```rust
#[serde(default)]
pub dns_tcp_connections: DnsTcpConnections,
#[serde(default)]
pub dns_dot_connections: DnsDotConnections,
```

with `pub type DnsDotConnections = DnsTcpConnections;` declared next to the
`DnsTcpConnections` struct (`engine.rs:97-101`). `#[serde(default)]` matches
every counter group added after the original set and keeps an older stored or
pushed payload deserializable. Field order fixes JSON order, so the payload
reads `dns_tcp_connections` then `dns_dot_connections`.

No other `fah-model` change. No business logic enters the crate (hard rule 2).

### D6 — Registry: the same shape as the TCP slot, named for DoT

**Decided.** In `crates/fah-metrics/src/registry.rs`:

- field beside `:66` — `pub(crate) dns_dot_connections: ArcSwap<fah_model::DnsDotConnections>`
- init beside `:116` — `ArcSwap::new(Arc::new(fah_model::DnsDotConnections::default()))`
- setter beside `:241` — `pub fn set_dns_dot_connections(&self, snapshot: fah_model::DnsDotConnections)`
- `engine_telemetry()` beside `:318` — `dns_dot_connections: **self.dns_dot_connections.load(),`

One `ArcSwap` per group, exactly as TCP and UDP do, so the three figures are
always read as one consistent value.

### D7 — Telemetry: one more parameter on the existing poll

**Decided.** `spawn_telemetry_poll` gains `dns_dot: Arc<fah_dns::DotConnectionGauge>`
after `dns_tcp` (`main.rs:1119`), the body gains
`metrics.set_dns_dot_connections(dns_dot.snapshot());` beside `:1129`, and the
call site at `:714` passes `dns.dot_connections()` after `dns.tcp_connections()`.

No new task, no new tick, no change to `TELEMETRY_POLL`. The figure therefore
inherits the documented "up to one interval old" caveat (API.md:390-393).

### D8 — JSON shape

```json
"dns_tcp_connections": { "active": 2, "peak": 9, "closed_oversize": 0 },
"dns_dot_connections": { "active": 1, "peak": 6, "closed_oversize": 0 },
```

`active` is a live gauge, `peak` a process-lifetime high-water mark that never
resets, `closed_oversize` a monotonic count of connections closed because a
2-byte length prefix exceeded `fah_dns::MAX_MESSAGE_LEN`. Identical semantics to
the TCP group — deliberately, so the soak can read both with one rule.

### D9 — CONTEXT.md is not touched

`dot` is already defined as a Client Transport (`CONTEXT.md:113-118`). This
change coins no term, so hard rule 6 does not fire. Recorded here so the review
does not ask for it.

## 4. File-by-file changes

1. **`crates/fah-model/src/engine.rs`** — `DnsDotConnections` alias; one
   `Counters` field with `#[serde(default)]` (D5).
2. **`crates/fah-dns/src/tcp.rs`** — `handle_connection`'s fifth parameter
   becomes `&TcpConnectionGauge`; drop the `if let Some(...)` around the
   `fetch_add`; the call at `:114` keeps passing `&open` (D4).
3. **`crates/fah-dns/src/dot.rs`** —
   - `pub type DotConnectionGauge = crate::tcp::TcpConnectionGauge;`
   - `run` and `run_with` take `gauge: Arc<TcpConnectionGauge>`; public `run`
     keeps it last (`run(listener, tls, pipeline, gauge)`), `run_with` keeps
     `handshake_timeout` last so the test harness signature reads unchanged
     except for the new argument.
   - in the accept loop: `let open = OpenConnection::enter(&gauge);` after
     `set_nodelay`, moved into the spawned task, `drop(open)` after
     `report_connection_end`, beside `_slot` (D3).
   - `serve_connection` takes `gauge: &TcpConnectionGauge` and passes it to
     `tcp::handle_connection` in place of `None` (D4).
   - the `use` list gains `fah_common::connections::OpenConnection`.
4. **`crates/fah-dns/src/server.rs`** — `dot_gauge` field, built in `bind()`,
   cloned into the DoT spawn in `serve()`, `pub fn dot_connections()` beside
   `tcp_connections()` (D2).
5. **`crates/fah-dns/src/lib.rs`** — export `DotConnectionGauge` on the `dot`
   re-export line (`:22`).
6. **`crates/fah-metrics/src/registry.rs`** — the four sites in D6.
7. **`crates/fastadhunter/src/main.rs`** — the three sites in D7.
8. **Tests** — §7.

Layering: `fah-dns` (L3) depends on `fah-model` and `fah-common` (L1) only; the
binary (L4) stays the only place that knows both `fah-dns` and `fah-metrics`
(the reason is already written at `main.rs:1131-1134`). `layering.rs` stays
green with no change.

## 5. Control flow after the change

```text
accept() on :853
  |- permit acquired before accept (unchanged)
     |- OpenConnection::enter(&dot_gauge)      active+1, peak = max(peak, active)
        |- spawn
           |- TLS handshake (counted while it runs — D3)
           |- tcp::handle_connection(..., Transport::Dot, &open)
           |     |- length prefix > 16 KiB     closed_oversize+1, close
           |- report_connection_end
           |- drop(open)                       active-1, peak unchanged
                                               drop(_slot) releases the permit

every 10 s: main.rs poll -> dot_gauge.snapshot() -> metrics.set_dns_dot_connections
            -> registry ArcSwap -> engine_telemetry() -> GET /api/v1/telemetry
```

## 6. Runtime, concurrency, performance, memory

- **Hot path:** DNS query handling is untouched. The added work is one
  `fetch_add` plus one `fetch_max` per accepted DoT connection and one
  `fetch_sub` when it ends — the cost TCP already pays, on a path that is
  already doing a TLS handshake. The per-message `len > MAX_MESSAGE_LEN` branch
  loses an `Option` check.
- **Memory:** one `Arc<TcpConnectionGauge>` per process (two `AtomicU32`, one
  `AtomicU64`), one more `ArcSwap` slot in the registry, 24 bytes more in the
  telemetry payload.
- **Concurrency:** all counters are `Relaxed` atomics read only by the 10 s
  poll; `active` and `peak` are maintained by the same RAII guard TCP uses, so a
  connection task that panics still decrements. No lock, no allocation.
- **Ordering caveat, inherited not introduced:** `snapshot()` reads three
  atomics separately, so `active` and `peak` can straddle a change. `peak` is
  clamped with `.max(active)` (`tcp.rs:35`), which is why the TCP figure has
  never shown `peak < active`; DoT inherits that.

## 7. Test strategy

### 7.1 `crates/fah-dns/src/dot.rs` — unit

Harness change: `listen()` (`dot.rs:350`) builds
`Arc::new(TcpConnectionGauge::default())`, passes a clone to `run_with`, and
keeps it on `Listener` as `gauge`. A poll helper reads a snapshot every 10 ms
for up to 2 s, because the decrement happens on the server task after the
client's FIN.

1. **`a_served_dot_connection_is_counted_and_released`** — open three
   connections with `client_accepting_any()`, exchange `a_query()` on each so
   they are certainly past the handshake, assert `(active, peak) == (3, 3)`;
   drop one and poll until `active == 2`, assert `peak` is still `3`; drop the
   rest and poll until `active == 0`, `peak` still `3`.
2. **`a_dot_frame_over_the_bound_counts_closed_oversize`** — mirror of
   `tcp.rs:331-354`: connect, write `(MAX_MESSAGE_LEN + 1) as u16`, read to EOF,
   assert `closed_oversize == 1`, poll until `active == 0`. This is the figure
   nothing else in the suite can see.
3. **`the_connection_bound_queues_the_next_client_until_a_slot_frees`**
   (`dot.rs:550`, extended, not replaced) — with the 64 connections held, assert
   `(active, peak) == (DOT_MAX_CONNECTIONS, DOT_MAX_CONNECTIONS)`; after the
   65th is shown to be queued, assert `peak` is still `DOT_MAX_CONNECTIONS` —
   never 65, because the permit is taken before `accept()` and the queued client
   is never accepted. This assertion is the one p3-10 B2's row rests on: it
   proves the gauge measures admitted connections, which is what the cap bounds.

### 7.2 `crates/fah-dns/tests/server_integration.rs` — wiring, both directions

One test, `each_dns_stream_listener_counts_into_its_own_gauge`: bind a `Server`
with `dot_enabled = true` and an explicit `tcp_max_connections`, serve it with
`Some(DotTls::new(store, fallback))` over a CA-generating `CertStore` in a
`tempfile::tempdir()` (`fah_certs`, `rustls` and `tokio-rustls` are already
dependencies of this crate, so the test target has them).

- open **one DoT connection only**, exchange a query, assert
  `dot_connections().snapshot().active == 1` **and**
  `tcp_connections().snapshot() == DnsTcpConnections::default()`;
- drop it, open **one plain TCP/53 connection only**, exchange a query, assert
  `tcp_connections().snapshot().active == 1`, and on the DoT gauge
  `active == 0` with `peak == 1` — the surviving DoT peak proves it was the DoT
  gauge that moved, not a shared one.

This is the test that fails if `server.rs` hands the same `Arc` to both
listeners.

### 7.3 `crates/fah-metrics/src/registry.rs` — round trip

`dns_dot_connections_round_trip_as_one_value`, mirroring `:683-703`: store a
busy value, read it back off `engine_telemetry().counters`, store an idle value,
read it back. Also assert that storing the DoT value leaves
`counters.dns_tcp_connections` at its default — the slot-swap mistake in D6 is
the one this catches.

### 7.4 `crates/fah-api/tests/api.rs` — payload contract

The fixture at `:426` gains the new group with values distinct from the TCP
ones, and the assertions at `:1142-1146` gain
`counters.dns_dot_connections.{active,peak,closed_oversize}` — pinned where
every other counter group is pinned.

### 7.5 `crates/fastadhunter/tests/shipped_path_e2e.rs` — the whole road

The test already resolves over DoT (`:167-186`) and already reads
`/api/v1/telemetry` (`:222-228`). After the DoT exchanges, poll
`GET /api/v1/telemetry` every 500 ms for up to 20 s until
`counters.dns_dot_connections.peak >= 1`, then assert that
`counters.dns_tcp_connections.peak` reflects only the plain-TCP exchange the
test already made.

Cost, stated because it is real: up to one `TELEMETRY_POLL` (10 s) added to one
test that already boots a full instance. Accepted, because this is the only test
that covers `main.rs`'s poll wiring — the one line whose failure mode is a soak
that quietly reports zeros for seven days. `peak` is asserted rather than
`active` precisely because it does not race the poll.

### 7.6 Mutation checks — apply, run, revert, record

Each mutation is applied locally, the suite is run, the named test is confirmed
to fail, and the mutation is reverted. The results go in the review file; a
claim that a test *would* fail is not evidence.

1. Pass a fresh `Arc::new(TcpConnectionGauge::default())` into `dot::run_with`
   instead of the `Server`-owned one — §7.1.1 and §7.2 must fail.
2. Delete the `OpenConnection::enter` line in `dot.rs`, keep the rest — §7.1.1
   and §7.1.3 must fail.
3. In `main.rs`, feed `dns_tcp.snapshot()` to `set_dns_dot_connections` — §7.5
   must fail on the TCP assertion.
4. In `server.rs`, return `tcp_gauge` from `dot_connections()` — §7.2 must fail.

### 7.7 Gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
```

No bench is owed: no hot path is touched (§6).

## 8. Documentation consequences — proposed, not written

**None of these is edited without its own explicit yes** (§Working agreement).
The implementer finishes the code, then lists them and waits.

- **API.md — required in the same change.** Three sites: the payload example
  (`:227`) gains the `dns_dot_connections` row; the prose at `:287-299` says
  today that the DoT listener "is handed no gauge" and that the 64-connection
  cap "has no counter and no config key" — the first half becomes false and is
  rewritten to describe the new group, the config-key half stays true; the poll
  note (`:391`) adds the new field to the list of values that can be one
  interval old.
- **`docs/project-state.md` §Risk inventory close-out** — the F1 follow-up that
  sets the final `dns.tcp_max_connections` must be told to read
  `dns_tcp_connections` alone, and that DoT now has its own figure. Proposed,
  separate yes.
- **`plan/wip/phase3/p3-10-post-merge-performance.md`** — B2's "peak concurrent
  DoT connections" row can name the field it reads. Proposed, separate yes.
- **`plan/wip/phase3/CLAUDE.md`** — row 10b status. Proposed, separate yes.
- **`docs/code-review/phase3/p3-10b-dot-connection-gauge-review.md`** — required
  by §TASK COMPLETION, and that requirement is its own permission.
- **CONTEXT.md** — not needed (D9). **CONFIGURATION.md** — not needed: no key.

## 9. Acceptance mapping

| Task-file criterion | Closed by |
| ------------------- | --------- |
| A DoT connection appears in the new gauge and in `/api/v1/telemetry` | §7.1.1, §7.5 |
| A TCP connection appears in the TCP gauge; neither leaks into the other | §7.2, §7.3 |
| A DoT oversize close increments `closed_oversize` on the DoT gauge | §7.1.2 |
| The test fails when the wiring is broken — verified, not assumed | §7.6 (the task file's literal `None` mutation no longer compiles — D4) |
| `fmt`, `clippy -D warnings`, `test --all-features` green | §7.7 |
| API.md updated in the same change, with its own approval | §8 |
| B2's "peak concurrent DoT connections" becomes runnable | §7.1.3 fixes the meaning of `peak`; §7.5 fixes the JSON it is read from |

## 10. Out of scope — binding

- `DOT_MAX_CONNECTIONS` keeps its value and stays compiled-in. Whether 64 is
  right, and whether it deserves a config key, is p3-10 A6 and B2's row.
- No default changes anywhere, `dns.tcp_max_connections` included.
- TCP gauge semantics untouched: same fields, same meanings, same JSON key. The
  only `tcp.rs` edit is the parameter in D4.
- No DoH gauge. DoH is `/dns-query` on the API listener
  (`crates/fah-api/src/routes.rs:125`), not a `fah-dns` listener; counting it is
  a different question about a different listener and is not part of A5.
- No rename of `TcpConnectionGauge` or `DnsTcpConnections` (D1).
- No new `Transport` variant, no new event field, no dashboard change.
- No deploy. This lands in the same build as row 10c, for p3-11's single deploy.
