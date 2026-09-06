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
| 4 | Low | CONFIGURATION.md, CONTEXT.md, API.md, ADR | New section, new term ("allocation domain"), config body gains `runtime`; no ADR for the topology. | **deferred**: each edit is its own go per the working agreement. |
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
- Docs, one go each: ADR, CONTEXT.md, CONFIGURATION.md `[runtime]`, API.md.
- Dashboard: `runtime.http_runtimes` in the settings metadata and types.
- Merge to `main`, tag, production image, owner-run swap; 7-day soak with
  N=2 as the predeclared verdict on the plateau.
- Only if the soak shows the plateau climbing: an idle-time reclamation hook on
  the domain threads (couples `fah-http` to the allocator; not before).
- Delete `fastadhunter-h1buf-db2f9b2-rosready.tar` from `kingston/`; remove the
  `E:/FastAdHunter-var-h1buf031` worktree.

**PASS WITH DEFERRED FINDINGS** (3, 4 deferred; 1, 2 fixed; 5–10 accepted).

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
| 14 | Low | API.md `/api/v1/debug/memory`; `GET`/`PATCH /api/v1/config` body | `cpu_user_ms` / `cpu_system_ms` (b254977) are not documented (grep: no hit); the config body's new `runtime` section is not documented either (part of finding 4). `concurrent_connections` is documented (l. 357, 398, 434). | **deferred**: doc edit, own go. |
| 15 | Low | naming: `[runtime]` section; `server.rs` log field `domain = index`; CONTEXT.md | (a) `config_store.rs` already uses "runtime key" to mean *live-applied* (test `a_runtime_key_applies_live_without_requiring_a_restart`); the new `[runtime]` section is the opposite, boot-only. (b) `tracing::error!(domain = index, "HTTP domain is not accepting…")` — in a DNS blocker's log a field named `domain` reads as a hostname. (c) CONTEXT.md has no "allocation domain" entry (finding 4). | **deferred**: owner's call. Suggest the CONTEXT.md entry state explicitly "not a domain name", the log field become `http_domain`, and CONFIGURATION.md `[runtime]` carry "boot-only, restart to apply". |
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
- Docs (14, 15, and 4 above): API.md `cpu_*` and `runtime`, CONTEXT.md "allocation domain", CONFIGURATION.md `[runtime]` boot-only note.

**PASS WITH DEFERRED FINDINGS** (11a, 12, 13, 16, 17 fixed; 11b open, optional; 14, 15 deferred docs; 18–20 accepted).
