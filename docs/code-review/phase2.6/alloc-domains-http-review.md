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
