# Review — HTTP allocation domain (`alloc-domains/http`)

Reviewed 2026-09-07 against `main` = `64be513`. Branch head `17468d4`; code
commits `0ed7d1e` (gauge cherry-pick), `14e0bdd` (allocation domain),
`b254977` (`cpu_*` on `/debug/memory`). Task and measurements:
[alloc-domains-http-task.md](alloc-domains-http-task.md) §Results.

## Summary

- Diff: 21 files, +723 / −57 under `crates/`. Gates green on the dev box
  (fmt, clippy `-D warnings`, `cargo test --all-features --workspace`).
- Design as decided by the owner on 2026-09-06: one acceptor on the base
  runtime, N `current_thread` runtimes on their own threads, one shared
  `max_connections` semaphore, one `ProxyCounters`, one `Proxy` per domain
  built on the domain's thread, `0` = the 0.3.1 shared-runtime path.
- Two findings fixed in this review (dead-domain rotation, `http_runtimes`
  upper bound), each with a test; the rest are doc or dashboard follow-ups.
- Hot path: the DNS path is untouched. The HTTP accept path gains one enum
  match, one `into_std`/`from_std` pair and one channel send per accepted
  connection; nothing per byte.
- Memory: N runtimes ≈ N threads (2 MiB virtual stack each, resident on
  touch) plus one hyper-util client and pool per domain. Measured on the
  RB5009: the held residue after 900 MiB drops from +56..+60 to +14..+17 MiB
  (task file §Results).

## Decisions

- `http_runtimes = 0` stays a product option: it is the A arm of every
  future A/B and the rollback without a rebuild.
- The library owns threads and runtimes (principle 6 deviation) because the
  hand-off and drain are one mechanism with the accept loop; the binary still
  decides N and the drain timeout.
- A domain that stops accepting is removed from the rotation, not treated as
  fatal: the DNS listener death path exits the process; here the surviving
  domains keep serving and the log carries an `error` line.
- Stop signal is a `watch`, not the acceptor's abort alone: joining domain
  threads from a `current_thread` runtime would otherwise deadlock on the
  abort's deferred drop (found in the tests, not on the device).
- The predeclared memory criterion is recorded as not met; adoption is the
  owner's decision, stated in the ADR, not inferred from this review.

## Findings

Severity-ranked. Outcome per finding.

| # | Severity | Site | Finding | Outcome |
| - | -------- | ---- | ------- | ------- |
| 1 | Medium | `crates/fah-http/src/server.rs` `Dispatch::Domains` | A domain whose thread died (panic in the factory or the loop) kept its sender in the rotation: every N-th connection was accepted, then dropped, forever, with one `warn` per drop. | **fixed**: on `SendError` the handoff is returned, the sender removed, the next domain tried; `error` once per removal, once more if none is left. Test `a_domain_that_fails_to_start_is_dropped_from_the_rotation`. |
| 2 | Low | `crates/fah-config/src/lib.rs` `validate` | `runtime.http_runtimes` had no upper bound: a typo such as `2000` spawns 2000 threads and runtimes at boot. | **fixed**: `MAX_HTTP_RUNTIMES = 64`, validation error `must be at most 64`; `0` and `64` accepted. Test `validation_rejects_too_many_http_runtimes`. |
| 3 | Low | `dashboard/frontend/src/pages/settings/metadata.ts`, `api/types.ts` | The settings page lists keys from its metadata table; `runtime.http_runtimes` is absent, so the key is invisible in the dashboard (the API returns it, PATCH accepts it). | **deferred**: frontend change with its own gates (`npm test`), not in this branch. |
| 4 | Low | CONFIGURATION.md, CONTEXT.md, API.md, ADR | New section, new term ("allocation domain"), config body gains `runtime`; no ADR for the topology. | **fixed** (docs pass, 2026-09-07): CONFIGURATION.md `[runtime]` + boot-class note, CONTEXT.md "Allocation Domain", API.md (`[runtime]` boot-only, `cpu_*_ms`), ADR-0006, ARCHITECTURE.md §Runtime Model exception bullet. |
| 5 | Info | `crates/fah-config/src/schema/runtime.rs` | The default is host-derived (`max(1, cores/2)`), so the TOML written on first boot pins the core count of the machine that wrote it. Moving the file to another device carries the number. | accepted; goes into CONFIGURATION.md `[runtime]` with the key. |
| 6 | Info | `crates/fah-http/src/server.rs` `Server` drop | Dropping a `Server` without `shutdown()` leaves the domain threads draining on their own (the `watch` sender drop is the signal) and never joined. | accepted: same contract as `fah_dns::Server` ("dropping does not stop it"); the binary always calls `shutdown()`. |
| 7 | Info | `crates/fastadhunter/src/main.rs` `Engine::shutdown` | Joins the domain threads on the `block_on` thread: main blocks for drain + runtime shutdown, ≤ ~6 s. | accepted: measured 5.5 s on the router with three transfers in flight, inside `stop-time=10s`. |
| 8 | Info | `crates/fah-http/src/server.rs` `Dispatch::Domains` | If an accept races the shutdown flip, the acceptor can log one spurious `not accepting` error before its abort lands. | accepted: shutdown only, one line. |
| 9 | Info | `crates/fah-http/src/domain.rs` `HANDOFF_QUEUE = 32` | Bounded channel per domain; a stalled domain blocks the acceptor after 32 queued sockets while the others idle (head-of-line). | accepted: the permit bound already holds the total; a domain that cannot drain 32 hand-offs is not serving anyway. |
| 10 | Info | benches | `cargo bench -p fah-http` not run: the change is per accepted connection and the proxy benches drive `serve_connection` over a duplex stream, not the accept loop. | not run; the RB5009 throughput and p95 in the task file are the measurement. |

## Measurements

None here. Dev-box smoke, RB5009 arms, extra waves and the in-flight stop test
are in [alloc-domains-http-task.md](alloc-domains-http-task.md) §Results.

## Files changed

Branch, committed: `crates/fah-config/src/{env.rs,lib.rs,schema/mod.rs,schema/runtime.rs}`,
`crates/fah-http/{Cargo.toml,src/lib.rs,src/proxy.rs,src/server.rs,src/domain.rs,src/connections.rs}`,
`crates/fah-model/src/{perf.rs,memory.rs,lib.rs}`, `crates/fah-api/src/wire.rs`,
`crates/fah-api/tests/api.rs`, `crates/fah-stats/src/{history/perf.rs,history/reader.rs,stats.rs}`,
`crates/fastadhunter/src/{main.rs,process.rs}`, `crates/fastadhunter/tests/history_e2e.rs`,
`API.md` (from the cherry-pick).

This review, uncommitted: `crates/fah-http/src/server.rs` (finding 1 + test),
`crates/fah-config/src/lib.rs` (finding 2 + test), this file.

Dev-box artefacts: `E:/fah-diag/tools/spikes.sh`, `E:/FastAdHunter/target/smoke.sh`,
image `fastadhunter:alloc-b254977` (amd64 local, arm64 tar on `kingston/`).

## Remaining TODOs

- Commit the two fixes and this file (go).
- Docs: done in the 2026-09-07 docs pass (finding 4).
- Dashboard: `runtime.http_runtimes` in the settings metadata and types.
- Merge to `main`, tag, production image, owner-run swap; 7-day soak with
  N=2 as the predeclared verdict on the plateau.
- Only if the soak shows the plateau climbing: an idle-time reclamation hook on
  the domain threads (couples `fah-http` to the allocator; not before).
- Delete `fastadhunter-h1buf-db2f9b2-rosready.tar` from `kingston/`; remove the
  `E:/FastAdHunter-var-h1buf031` worktree.

**PASS WITH DEFERRED FINDINGS** (3 deferred; 1, 2, 4 fixed; 5–10 accepted).

---

# Second review — independent (2026-09-07)

Reviewed the working tree = branch head `0b55d32` against `main` = `64be513`
(the two "uncommitted" fixes from the first review are committed in `0b55d32`;
`git status` shows no crate changes). Findings 1–10 above read once, not
repeated. Numbering continues at 11.

## Summary

- Gates re-run on the dev box (Windows), filtered: `cargo fmt --check` OK,
  clippy `-D warnings` finished clean, `cargo test --all-features --workspace`
  0 failed (28 suites; 5 ignored in one integration suite). `cargo bench` not
  run.
- Owner decisions all present in code: one acceptor + N bounded channels (no
  `SO_REUSEPORT`), one shared `max_connections` semaphore, one
  `current_thread` runtime per std thread, `0` = 0.3.1 path, one `Proxy` per
  domain built on the domain thread, shared `Arc`s, cloned events `Sender`,
  `fah-http` owning threads. Cherry-pick `0ed7d1e` is its own first commit.
- Accept path: permit → accept → `set_nodelay` → gauge `enter()` → `into_std`
  → channel send; the permit and gauge guard travel inside `Accepted` and drop
  with the connection task on the domain (or on the base thread if the
  hand-off fails, releasing both). Nothing per byte. DNS path untouched.
- Two behavioral changes the task file does not state: `Engine::shutdown` now
  blocks up to ~6 s with DNS already aborted (F11); `PATCH /api/v1/config`
  misreports `restart_required` for the new key (F12). Both are small code
  changes, fixed in this pass (11a, 12).
- No new Rust comments (the two `+//` lines in `server.rs` are the 0.3.1 Nagle
  comment moved from the spawned task to the accept loop). `fah-model` gained
  data only. Layering unchanged (no new manifest dependencies).

## Findings

Severity-ranked. Outcome per finding.

| # | Severity | Site | Finding | Outcome |
| - | -------- | ---- | ------- | ------- |
| 11 | Medium | `crates/fastadhunter/src/main.rs` `Engine::shutdown` (l. 589–594); `crates/fah-http/src/domain.rs` drain; `proxy.rs:298` | Order is `dns.shutdown()` (abort, instant) → `http.shutdown()` (join, up to 5 s drain + 1 s runtime) → `api`. Before this branch all three were instant aborts, so the order was free; now DNS is down for the whole HTTP drain. The drain waits for connection *tasks to end*, not for in-flight exchanges: `serve_connection` never calls hyper's `graceful_shutdown`, `keep_alive(true)` is set, and hyper 1.10.1 closes an idle keep-alive connection only when `header_read_timeout` (10 s default) fires (`proto/h1/conn.rs:219–233` starts that timer on every wait for a head, idle included) or the client closes. Any browser keep-alive connection idle < 5 s at stop therefore holds the drain to its full 5 s. Net: every restart/upgrade with an idle HTTP connection costs up to 5 s more DNS outage for the household. The router test (three transfers in flight, 5.0 s) is consistent; the idle-only case was not measured. | **fixed** (a): `Engine::shutdown` joins HTTP before aborting DNS (`main.rs`), so DNS answers through the drain. (b) open, optional, larger: a stop `watch` into `Proxy::serve_connection` that calls `Connection::graceful_shutdown` (disables keep-alive, finishes the in-flight exchange), so the drain ends when responses complete rather than when connections happen to close. |
| 12 | Medium | `crates/fah-api/src/config_store.rs` `BOOT_KEYS` (l. 35–58) | `runtime.http_runtimes` is read once in `Engine::start` (`main.rs:397`) and never live-applied, but `runtime` is not in `BOOT_KEYS`, so `PATCH /api/v1/config {"runtime":{"http_runtimes":N}}` persists and answers `restart_required: false`. Contradicts the store's own contract (file header: boot keys answer `true`) and the reasoning recorded there for `http` / `egress`. | **fixed**: `"runtime"` (whole section) in `BOOT_KEYS`; `runtime.http_runtimes` added to `boot_key_classification_matches_what_actually_applies_the_key`. |
| 13 | Low | `crates/fah-http/src/domain.rs` `spawn_domain` | The `current_thread` runtime is built in the parent — inside the base runtime's `block_on` — and moved into the thread closure. If `std::thread::Builder::spawn` fails (thread limit, EAGAIN) std drops the closure, and dropping a `tokio::runtime::Runtime` inside an entered runtime context panics: tokio-1.53.0 `runtime/blocking/shutdown.rs:52` "Cannot drop a runtime in a context where blocking is not allowed" via `context/blocking.rs:20` (`is_entered()` is true inside `block_on`). So the one error branch `serve_domains` exists to report becomes a boot-time panic instead of an `io::Error`. Rare; boot only; untestable as written. | **fixed**: the runtime is built inside the thread; the build result comes back over a `std::sync::mpsc::sync_channel(1)` and `spawn_domain` returns the `io::Error`. Also closes 19(a). |
| 14 | Low | API.md `/api/v1/debug/memory`; `GET`/`PATCH /api/v1/config` body | `cpu_user_ms` / `cpu_system_ms` (b254977) are not documented (grep: no hit); the config body's new `runtime` section is not documented either (part of finding 4). `concurrent_connections` is documented (l. 357, 398, 434). | **fixed** (docs pass): API.md `/debug/memory` example + field note (`getrusage` `ru_utime`/`ru_stime`, ms, cumulative, `null` off Unix, not in `/telemetry`); `[runtime]` added to the boot-only list under `POST /api/v1/config`. |
| 15 | Low | naming: `[runtime]` section; `server.rs` log field `domain = index`; CONTEXT.md | (a) `config_store.rs` already uses "runtime key" to mean *live-applied* (test `a_runtime_key_applies_live_without_requiring_a_restart`); the new `[runtime]` section is the opposite, boot-only. (b) `tracing::error!(domain = index, "HTTP domain is not accepting…")` — in a DNS blocker's log a field named `domain` reads as a hostname. (c) CONTEXT.md has no "allocation domain" entry (finding 4). | **fixed** (docs pass): CONTEXT.md "Allocation Domain" states "not a domain *name*"; the log field is `http_domain` (`server.rs`); CONFIGURATION.md §Mutability classes names `[runtime]` **boot** with the reason. The section name stays `[runtime]`. |
| 16 | Low | `crates/fah-config/src/env.rs` | `["runtime","http_runtimes"]` arm and `coerce_usize` have no test; every other env key family has one in `lib.rs` tests. The router arms are switched by exactly this env var. | **fixed**: test `runtime_env_override_applies_and_is_validated` (`=2` → 2, `=x` → error, `=65` → validation error after `apply_env_overrides`). |
| 17 | Low | `server.rs` test `a_domain_that_fails_to_start_is_dropped_from_the_rotation` | Relies on a 200 ms sleep for the panicking thread to drop its inbox. If it has not (loaded box), the first send to it succeeds (queue depth 32), the socket is dropped with the receiver, the client reads an empty body and the `" 400 "` assertion fails. Which index panics is also scheduling-dependent (first factory *call*, not index 0), though the rotation handles either order. The thread panic prints to stderr outside test capture. | **fixed**: the sleep is replaced by a bounded wait (5 s) until exactly one entry of `server.domains` reports `is_finished()`, which is when the dead domain's inbox is gone. |
| 18 | Info | `server.rs` test `shutdown_lets_an_in_flight_request_finish` | Uses HTTP/1.0, so the connection ends by itself after the response. It proves "drain waits for a connection to end", not "drain lets an in-flight exchange finish then stops" — with HTTP/1.1 (the production case) the same test would sit in the drain until the timeout (finding 11). | noted; becomes the regression test for 11(b) if that lands. |
| 19 | Info | cross-thread drops not in the task file §Results list | (a) The domain runtime's internals (driver, timer, queues) are allocated on the base thread in `spawn_domain` and dropped on the domain thread at exit. (b) The hand-off channel's block list is allocated by the base-side sender on push and recycled to the sender's free list by the domain receiver; the last holder frees it at shutdown. Both one-time, shutdown only. No per-connection cross-thread free found: `Accepted` carries an fd, a `SocketAddr`, a permit (atomics on a shared semaphore) and an `Arc` guard — no owned heap. | accepted. |
| 20 | Info | `crates/fastadhunter/src/main.rs` `spawn_perf_sampler` | `https_connections` is `None` at its only call site (Phase 3 hook from the cherry-pick). `https` is 0 in the sample, the wire type, the API test fake and API.md. | accepted; principle 5 note only. |

## Scope checklist — what was checked, what was found

1. `server.rs` — permit acquired before `accept()` and held to task end in both `Dispatch` arms; gauge guard enters after `set_nodelay`, travels in `Accepted`, drops with the task, and on every early return (`into_std` / `from_std` failure, empty rotation) drops on the spot with the permit. `set_nodelay` precedes `into_std`. Rotation: `index = next % len; next = (index+1) % len`; on `SendError` `remove(index)` then recompute against the new `len` — no out-of-range, no modulo by zero (`%` only inside `while !senders.is_empty()`), one domain skipped once after a removal. Partial `serve_domains` failure: listener already taken and dropped (port released), spawned threads sit in `self.domains` and exit on sender drop, `start` fails via `?` — except the panic in 13. Shutdown: abort, `send_replace(true)`, join. From a `current_thread` test runtime the join blocks the only thread, but a domain's stop path (`inbox.close()`, drain, `JoinSet`) never needs it; the tests keep connections HTTP/1.0 so the drain ends without the client. No deadlock found.
2. `domain.rs` — runtime built in the parent, moved in (13). `select!`: `recv` / `changed` / `join_next` are all cancel-safe; `changed()` matched with `_` covers `Err` on sender drop (Server dropped without `shutdown`, finding 6); no lost wakeup (`subscribe()` precedes `send_replace`); no busy loop (the reap branch is gated on `!is_empty()`, and without the gate `select!` would just disable it); `JoinSet` bounded by permits and reaped one per iteration. `inbox.close()` then `recv()` to `None` serves what was queued. Drain → `warn` → `tasks.shutdown()` → `shutdown_timeout(1 s)`. hyper-util pool driver and idle-reaper tasks spawn through `TokioExecutor` = `tokio::spawn` from inside the connection task, so they land on the domain runtime and die at its shutdown; `Client::builder().build()` spawns nothing at build time, and IP-literal targets skip hyper-util's `spawn_blocking` resolver. `Proxy` built and last-dropped on the domain thread. Cross-thread drops: 19.
3. `main.rs` — factory *closure* built before the privilege drop (captures `Arc`s and copied config values only); *called* after it, on the domain threads (or once on the base runtime for N=0), and the threads are spawned after the drop. `NonZeroUsize::new` → `None` = shared path. `proxy_counters` still reach `spawn_telemetry_poll` via `unzip()`; the gauge reaches `spawn_perf_sampler`. Shutdown ≤ ~6 s inside `stop-time=10s` (measured 5.5 s); ordering: 11. Both `tracing::info!` lines carry `http_runtimes = N`.
4. `fah-config` — `#[serde(deny_unknown_fields, default)]` matches every sibling section; `[runtime]` serializes between `[engine]` and `[dns]`; host-derived default pins the writer's core count (finding 5); env arm + `coerce_usize` present, untested (16); `MAX_HTTP_RUNTIMES = 64` validated after env overrides; compiled-in default present. `restart_required`: 12.
5. `Proxy::with_counters` replaces the `ProxyCounters` `Proxy::new` allocates — one throwaway `Arc` per domain at boot, fine. `rt` + `macros` are the two features the code needs. `lib.rs` exports `ConnectionGauge` only. No manifest dependency added; `layering.rs` in the green run. `fah-model`: `ConcurrentConnections` and two `u64`s, data only.
6. Gauge — `enter`: `fetch_add` then `fetch_max`; `take_peak`: `swap(0)` then re-seed with `open()` — a high-water mark per sampling interval that starts the next interval at what is still open; a racing `enter`/drop can seed one high, harmless. Relaxed ordering is right for a gauge. `https` = 0 everywhere (20). Persisted samples without the field parse via `#[serde(default)]` on both `PerfSample` and `SlimPerfSample`.
7. `timeval_ms` — `tv_sec`/`tv_usec` are non-negative for rusage; saturating mul/add; no overflow. `MemoryResponse` `Option<u64>` serialize as `null` off Unix, matching `minor_page_faults`. API.md: 14.
8. Tests — 13 `#[tokio::test]` in `server.rs`; four use 100–200 ms sleeps (typical, low risk); 17 is the one with a real race. `Ok(0) | Err(_)` is the right tolerance (FIN on Linux, reset possible on Windows); the 5 s read window cannot be satisfied by the header timeout after `shutdown()` because the domain runtime is gone by then. 18 for what the in-flight test does not exercise.
9. Hard rules — no new comments; no lock/allocation/regex per query or byte (per accept: one `Arc` clone, two atomics, `into_std`/`from_std` = two reactor syscalls, one channel send); bounded: ≤ 64 threads, 32-deep channels, `JoinSet` ≤ `max_connections`. Vocabulary: 15.

Plan compliance: every owner decision implemented as specified; no scope beyond the task; the two undocumented deviations are 11 (shutdown blocks, DNS first) and 12 (API contract). Architecture: layering intact; the only new abstraction is `Dispatch`, which the two paths need. Performance: nothing added per byte or per query. Memory: per domain one thread stack (resident on touch), one runtime, one hyper-util pool, one `Proxy`; all bounded by config.

## Measurements

None new. Gates as in Summary; device numbers stay in the task file.

## Files changed

Fix pass (uncommitted): `crates/fastadhunter/src/main.rs` (11a),
`crates/fah-api/src/config_store.rs` (12), `crates/fah-http/src/domain.rs` (13),
`crates/fah-config/src/lib.rs` (16), `crates/fah-http/src/server.rs` (17);
5 files, +53/−6. Gates re-run after the fixes: fmt OK, clippy `-D warnings`
clean, `cargo test --all-features --workspace` 0 failed (32 suites).

## Remaining TODOs

- Commit the five fixes and this file (go).
- 11(b) `graceful_shutdown` through `Proxy::serve_connection` — decide before the 7-day soak, since it changes what the soak's restarts cost.
- Docs (14, 15, 4): done in the docs pass, including ARCHITECTURE.md §Runtime Model and ADR-0006.

**PASS WITH DEFERRED FINDINGS** (11a, 12, 13, 14, 15, 16, 17 fixed; 11b open, optional; 18–20 accepted).

---

# Third pass — one proxied request, end to end (2026-09-07)

Question: does the HTTP code hold live objects after transfers finish, or is
the held residue (+14..+17 MiB at N=2, +56..+60 at N=0, task file §Results)
allocator page state? Read: `server.rs`, `domain.rs`, `proxy.rs`,
`request.rs`, `claim.rs`, `connections.rs`, `main.rs` fan-out,
`adapters.rs`, `fah-dns/src/upstream/{mod,encrypted}.rs`, `fah-stats`
`record_http`; hyper 1.10.1 `proto/h1/io.rs`, `body/incoming.rs`;
hyper-util 0.1.20 `client/legacy/pool.rs`. Numbering continues at 21.

## Stage trace

"Thread" is where the allocation is made / freed. N=0 = one multi-thread
runtime (work-stealing workers); N=2 = base runtime + `current_thread`
domain runtimes.

| Stage | Objects | Freed where (N=0 → N=2) | Bounded by |
| ----- | ------- | ----------------------- | ---------- |
| accept (`accept_loop`) | `TcpStream` (fd), `SocketAddr`, `OwnedSemaphorePermit` (Arc + count), `OpenConnection` (Arc clone). No heap. | worker → domain thread, at task end | `max_connections` |
| permit | taken before `accept()`; moved into `Accepted`, then into the connection task as `_permit`; released by drop after the hyper `Conn` is gone. Early returns (`into_std` / `from_std` failure, empty rotation) drop it on the spot. | same as the task | — |
| hand-off (`Dispatch::Domains`) | `Accepted` in a 32-slot `mpsc`; tokio recycles the channel block to the sender's free list, no per-accept free. | n/a | 32 × N |
| `serve_connection` | `service_fn` closure (`Arc<Proxy>` clone); hyper server `Conn`: read `BytesMut` 8 KiB → ≤ 408 KiB adaptive (`io.rs:223` reserves the next size; `record` only lowers the *next* reserve, never shrinks the allocation), write `BufList` ≤ 16 bufs, 8 KiB header cursor. | worker → domain, when the connection ends (client close, HTTP/1.0, or the 10 s `header_read_timeout` on an idle keep-alive) | `max_connections` × ≤ ~0.8 MiB |
| head → `judge` | `authority` (host clone), `url`, `host`, `path`, `method` `String`s; `Judged`. `authority`/`url` die at the end of `judge`, `Judged` at the end of `handle`. | same thread | per request |
| resolve (`approved_address`) | `claim.host.clone()` → `resolve_host`: hickory `Name`, query `Message`, reply, `Vec<IpAddr>`. UDP: the whole future runs on the caller, all on one thread. DoT/DoH: the reply comes from the exchange's background task over a channel. | UDP: same thread. DoT/DoH: exchange runtime → domain (finding 21) | per request |
| upstream (`client.request`) | pool key `(scheme, ip:port)`; new connection: `LiteralConnector` `Box<dyn Future>` once, `TcpStream`, hyper client `Conn` (read `BytesMut` ≤ 408 KiB fills with the response body) driven by a task spawned via `TokioExecutor` on the **current** runtime. | N=0: any worker. N=2: the domain | pool: 8 idle/host, 60 s |
| response/body | upstream `Incoming` passed straight (`Either::Left`). Body chunk = `Bytes` slice of the upstream read buffer, over hyper's `mpsc::channel(0)` + `want`: ≤ 1–2 chunks in flight. Chunk allocated by the upstream conn task, dropped by the server conn after the write. | N=0: **upstream-task worker → server-task worker, per chunk, per 408 KiB reallocation**. N=2: same thread | backpressure |
| events (`emit`) | `Box<RequestEvent>`: `Request { host, path, method: String }`, `Verdict` (`Arc<str>` clones), `policy: Option<Arc<str>>`. `try_send` into the 4096-slot base channel; `stats.record_http` consumes and drops; hub clone only with WS subscribers. | domain → base fan-out worker, per request (~300 B) | 4096 events |
| close → task end | hyper `Conn` dropped (buffers), `Arc<Proxy>` clone, then `_open`, `_permit`. `JoinSet` keeps only the output slot until the next `select!` turn reaps it; the future is already gone. | same thread | — |
| pool idle | `Idle { PoolClient(SendRequest), idle_at }` per entry; the conn task with its ≤ 408 KiB read buffer stays alive until the entry is dropped (`clear_expired` every `idle_timeout`, also removes empty keys; `put` refuses beyond `max_idle_per_host`). | domain (N=2) | 8/host × ≤ 0.4 MiB, 60–120 s |
| shutdown/drain | `tasks.shutdown()` drops every connection future (buffers, permit, guard); `runtime.shutdown_timeout` drops the pool tasks; `Proxy` (client + pool) last, on the domain. | domain | shutdown only |

Error paths: refused claim / blocked / refused destination / upstream error
all return a `Full<Bytes>` response (static or one `String` → `Bytes`) and
drop the request `Incoming`; no path keeps the head, the body, or the
`Judged`. A hyper connection error only bumps a counter. A panic in the task
is caught by the `JoinSet` and drops everything the task owned.

## Findings

| # | Severity | Site | Finding | Outcome |
| - | -------- | ---- | ------- | ------- |
| 21 | Low (Medium with a DoT/DoH upstream) | `crates/fah-dns/src/upstream/encrypted.rs` `acquire` → `connect` | The exchange (re)connect runs inside the *calling* task, and hickory's `TokioRuntimeProvider` spawns the exchange's background I/O with `tokio::spawn` on the **current** runtime. With N ≥ 1 and a DoT/DoH upstream, an HTTP-triggered resolve that finds the slot empty (first use, or after a generation bump on error) pins that upstream's multiplexed exchange to an HTTP domain runtime: every DNS-pipeline query through it then crosses base → domain → base, its reply buffers are allocated on the domain thread and freed by base workers (the F29 pattern, per DNS query, for as long as the exchange lives), and the exchange dies with the domain at shutdown. Under N=0 the spawn site did not matter. Default config is UDP, where the resolve future is self-contained and this does not apply. Not exercised on the probe (no DNS traffic). | **fixed**: `ExchangeConn::new` captures `Handle::try_current()`; `connect` spawns the owned connecting future on that handle (aborted on timeout) and falls back to inline when built outside a runtime. `ConnectTarget` derives `Clone`; `fah-dns` gains tokio `rt`. Test `the_exchange_lives_on_the_runtime_that_built_the_conn_not_the_caller`: connects from a throwaway `current_thread` runtime, drops it, then sends a query from the builder's runtime — fails at the `expect` with `runtime: None`, passes with the fix. Principle 6 trade accepted: hickory already spawns from inside the library; the handle only pins where. |
| 22 | Info | `proxy.rs` `emit` / `judge` | Per request, three `String`s and one `Box` are allocated on the domain thread and freed on the base fan-out worker. Small-object pages only (~300 B/request), reclaimed as soon as the domain allocates again, bounded by the 4096-slot channel. Not measurable in the A/B. | accepted; the only per-request cross-runtime heap traffic left. |
| 23 | Info | hyper `Buffered` read buffers | The adaptive read strategy grows a connection's `BytesMut` to 408 KiB on full reads and never shrinks it in place; a keep-alive server connection that carried a large upload, or a pooled upstream connection that carried a large download, holds up to 408 KiB until it closes (≤ 10 s idle server-side, ≤ 60–120 s pooled). Bounded by `max_connections` and 8/host; the 408 KiB buffer A/B showed the size does not drive the held residue. | accepted. |

## Answers

1. **Concrete live-object retention bug in the HTTP code: none found.** Every per-connection object has one owner whose drop is the connection task's end; the permit and gauge guard are locals of that task and drop on every path including abort. No `Arc` cycle: `Proxy` → `Client` → pool → idle `SendRequest`s; conn tasks hold IO and channel ends, never `Proxy` or the `JoinSet`. No channel retains streams, frames or responses beyond the 32-slot hand-off and the 4096-slot events channel, both drained continuously. Nothing request-scoped is promoted into `Proxy` (config, shared `Arc`s, the client, atomic counters only) or into the domain loop (`JoinSet`, `Arc<Proxy>`).
2. **Pool / task / channel lifetimes that could explain tens of MiB 15 min after the transfers: no.** The only lifetimes beyond a connection are pool idle entries (≤ 8/host × ≤ 0.4 MiB, gone by 120 s — the −2.6 / −1.3 MiB steps at +120 s in the task file are exactly this), idle keep-alive server connections (≤ 10 s), the events channel (≤ 4096 × ~300 B transient) and the `JoinSet`'s finished slots (reaped on the next turn). Ceiling of everything the code can still own 15 min later with no connection open: well under 1 MiB.
3. **N=2 changes drop locality materially.** At N=0 the server conn task and the upstream conn task are two tasks on a work-stealing pool: every body chunk (`Bytes` slice of the upstream read buffer) and every 408 KiB buffer reallocation is allocated on one worker and freed on another — the large remote frees F29 names, on workers that park after the burst. At N=2 both tasks, the pool and its reaper run on one `current_thread` runtime, so every body-path allocation is freed by its owner thread. What still crosses is listed in 4. Consistent with the residue falling from +56..+60 to +14..+17 MiB.
4. **Exact base ↔ domain traffic**, per direction: base → domain per accept: `Accepted { std TcpStream, SocketAddr, OwnedSemaphorePermit, OpenConnection }`, no heap. Domain → base per request: `Box<RequestEvent>` (three `String`s, `Arc<str>` clones), freed by the fan-out worker. Domain → base per connection end: semaphore release (atomic + waker of the acceptor), gauge decrement (atomic). DNS runtime → domain per resolve, DoT/DoH only: the reply `Message` over the exchange channel (UDP is self-contained on the domain). Shared read-only `Arc`s: `UpstreamPool`, `dyn Ruleset` (an old `Arc<Matcher>` is freed by whichever thread drops it last — can be a domain thread, once per reload), `PolicyState`, `ProxyCounters`. Shutdown only: channel halves, the `Proxy` with its pool. Plus finding 21 when it triggers.
5. **The remaining RSS is more consistent with allocator page retention than with live objects.** Code: the ceiling in 2. Prior measurement: the counting-allocator arms (resoak-0.3.1 diagnosis §Summary) show live bytes back at baseline after every burst while RSS keeps the step, and musl keeps +1.3 MiB where mimalloc keeps +22. The buffer A/B: halving hyper's buffers moved nothing. Behaviour in the task file: one small request returns nothing (−0.7) while a wave returns 3.6–4.9 MiB — reclamation only when the owning thread allocates again, which is mimalloc's collect-on-allocation model, not an object lifetime. And the residue tracks thread geometry (4× smaller when the body path is confined to one thread; scales with worker count in F29), which object lifetimes do not.
6. **Site to fix before relying on the architecture: finding 21** (DoT/DoH exchange pinned to a domain runtime). Not a memory bug by itself, but it silently moves a DNS-pipeline component onto an HTTP runtime and re-creates cross-runtime frees on the DNS path. Nothing else in the HTTP path needs a change; 22 and 23 are bounded and accepted.
7. **Stated explicitly: no concrete object-retention bug was found in the HTTP path.** The evidence points to allocator retention because (a) the code's post-transfer ownership ceiling is < 1 MiB, (b) the counting allocator already showed live bytes returning to baseline while RSS did not, (c) the held amount responds to *thread geometry* (N=0 vs N=2, worker count) and to *allocation activity on the owning thread* (a wave returns memory, a single request does not), neither of which a live-object leak does, and (d) buffer size, the one live-object knob on this path, was A/B'd without effect. The discriminating next experiment is the one already in the TODOs: an idle-time collect on the domain thread once its `JoinSet` is empty. Unlike F17 (collect on park, at 32 workers, ran before the remote frees arrived), at N=2 the frees are local, so a collect there tests page retention directly.

## Measurements

None new; references above are to the task file §Results and the 0.3.1
diagnosis.

## Files changed

Fix 21 (uncommitted): `crates/fah-dns/Cargo.toml`, `crates/fah-dns/src/upstream/encrypted.rs`;
2 files, +75/−13. Gates after the fix: fmt OK, clippy `-D warnings` clean,
`cargo test --all-features --workspace` 0 failed (31 suites).

Docs pass (uncommitted): CONFIGURATION.md (`[runtime]`, §Mutability classes),
CONTEXT.md ("Allocation Domain"), API.md (`[runtime]` boot-only, `cpu_*_ms`),
ARCHITECTURE.md (§Runtime Model), `docs/decisions/0006-http-allocation-domains.md`;
`crates/fah-http/src/server.rs` log field `http_domain` (finding 15).

## Remaining TODOs

- Commit fix 21, the docs pass and this file (go).
- Idle-time collect on the domain threads as the page-retention discriminator (already listed above; run it as an arm, not as a product change).

**PASS WITH DEFERRED FINDINGS** (21 fixed; 22, 23 accepted; earlier verdict unchanged).
