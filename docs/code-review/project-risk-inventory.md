# Project risk inventory — `main` at `baa2ecd`, 2026-09-11

**This is an inventory, not a backlog.** Nothing here is scheduled, and nothing
here is a finding against a task. It records where the code on `main` is
hardest to be sure about, so that a future bug has a place to be looked for
first.

## Method and limits

Static survey only: source read, `git` metadata, and the repo's own review
record under [docs/code-review/](.). No profiling, no fault injection, no
device run, nothing executed against production. Ranking is by blast radius ×
subtlety × how hard the failure is to catch in a test — not by line count.

Lenses: hot-path work (allocations, locks, syscalls, logging), memory lifetime
(tasks, connections, buffers, growth under repeated failure), lifecycle
(shutdown, cancellation, task death) and Rust hygiene (ownership, `unwrap`/
`expect`, `Send`/`Sync`). No code was changed and no benchmark was run.

## Inventory

### 1. `fah-rules/src/lifecycle/mod.rs` — 3436 lines, 24 public methods

Network fetch, disk cache, parse, compile, atomic swap, scheduler, policies and
user rules in one object. Six lock fields — `compile_lock` and per-list
`refresh_lock` (`tokio::sync::Mutex`), `status`, `last_attempted`,
`pending_cache` (`std::sync::Mutex`), `entries` (`std::sync::RwLock`) — taken
at 36 call sites in non-test code (before `#[cfg(test)]` at line 1386); **none
is on the per-query path**, which reads only the `ArcSwap<Matcher>`
(`matcher()`, line 521). Compile runs in `spawn_blocking` (line 1268), list
validation in another (line 764). One documented best-effort path: a `/data`
cache-write failure keeps the raw text in `pending_cache` and applies the rules
anyway (`commit_raw`, line 1039). 61 tests is a lot until the interleavings
are counted: scheduler refresh against an API refresh, remove-during-refresh,
compile-lock queued behind a slow download. Owns the data DNS blocking depends
on.

Non-test has one spawn (`spawn_scheduler`, line 1072, the `tokio::spawn` at
1074). `ListStatus` (line 232) has no in-progress state, so an aborted or
panicked refresh leaves nothing stuck; the manual API refresh is a detached
`tokio::spawn` (`fah-api/src/routes.rs:788`), not awaited by the handler, so a
client disconnect cannot cancel it mid-download.

### 2. `fah-dns/src/upstream/` — 4090 lines across mod/health/encrypted/plain, plus `alarm.rs`

Adaptive selection and health scoring are statistical, so "correct" is a
distribution and every test is a timing test. Coverage is heavy (mod 42,
health 41, encrypted 17, alarm 5 inline, plus `tests/adaptive_behaviour.rs`)
precisely because this is where heisenbugs live.

All 12 spawn/select sites in `mod.rs` are inside `#[cfg(test)]` (starts line
580); non-test `mod.rs` and `health.rs` use atomics only. The one lock is
`encrypted.rs:47` — a `tokio::sync::Mutex<Slot>` per encrypted upstream, held
across the TLS connect on purpose (one handshake for a stampede of first
queries) and released before the exchange itself, which is a cloned
multiplexer handle. The one non-test spawn in the module is
`encrypted.rs:196` (`runtime.spawn(connecting)`, per connect, aborted on
timeout) — not per query.

### 3. `fastadhunter/src/main.rs` — 1572 lines, 35 commits since June

Highest churn in the tree. Owns clocks, timers, wiring and shutdown ordering.
Teardown is `Engine::shutdown` at `main.rs:589`: http → dns → api → abort every
long-lived task (`Engine::tasks` collects all of them, including SWR workers
and the cache sweep); the DNS listener tasks are aborted in
`fah-dns/src/server.rs:107`. The runtime is then dropped at the end of
`main` — per-connection DNS-TCP and HTTP tasks end with it, HTTP domains drain
under `HTTP_DRAIN_TIMEOUT` (5 s). Process teardown has no test that can fail
(the e2e harness only `kill()`s), so regressions land silently and surface as
a container that will not stop cleanly. 11b (graceful shutdown of keep-alive
connections) is still open in
[alloc-domains-http-review.md](../phase2.6/alloc-domains-http-review.md).
Three teardown findings below: no stats flush on a clean stop (F10),
long-lived task death never observed (F11), runtime drop unbounded by a
`shutdown_timeout` (F12).

### 4. `fah-dns/src/cache.rs` — 1832 lines, 36 inline tests

Listed for consequence, not quality: carefully bounded, two limits,
incremental byte accounting, sharded. It is the invariant that keeps a 1 GB
router alive, and its `std::sync::Mutex<Shard>` × 16 sits on the per-query
path, which reads against hard rule 3's flat "no locks on the hot path".

The two bound tests are `filling_past_capacity_evicts…` and
`a_flood_of_large_answers_plateaus_at_the_byte_cap`. Critical sections are
short: a lookup is a `HashMap` get, two `Instant` compares and an `Arc` clone;
an insert is two `Box<str>` key clones, the map insert, the eviction loop and
`compact`. `stats()` (line 694) is the one caller that walks every entry of
every shard under its lock — every `history.sample_interval_seconds` (default
60 s) from the perf sampler and on demand from the API; ≤625 entries per shard
at the default `max_entries` of 10,000.

### 5. `fah-config/src/tz.rs` — POSIX TZ and DST, 576 lines, 10 tests

Rules like `M2.5.0,M11.1.0` are where date code stays wrong for years. A DST
error fires schedules an hour off twice a year and nothing crashes.

## Findings

Severity: **blocker** = must be fixed before the next merge (none found);
**material** = worth a task before Phase 3 closes; **minor** = record only.
Class: **defect** = confirmed against source; **recommendation** = a
judgement, unmeasured.

| # | Severity | Class | Where | Finding |
| --- | --- | --- | --- | --- |
| F1 | material | **fixed 2026-09-11, soak pending** | `fah-dns/src/tcp.rs` `run`, `MAX_MESSAGE_LEN`, `TcpConnectionGauge`; `fah-common/src/connections.rs` `ConnectionGauge` (shared with `fah-http`); `fah-config` `[dns] tcp_max_connections` | Was: DNS-over-TCP had no connection ceiling — one task per accept, bounded only by `TCP_IDLE_TIMEOUT` (10 s) and the fd limit — and each message allocated `vec![0u8; len]` from the client's own 2-byte length (≤64 KiB). Now: `[dns] tcp_max_connections` (boot, default 1024, mirrors `[http] max_connections`) sizes a semaphore whose permit is taken before `accept`, so a burst queues in the kernel backlog; a length prefix above the hardcoded 16 KiB `MAX_MESSAGE_LEN` closes the connection without allocating. The gauge (`active`, lifetime `peak`, `closed_oversize`) reaches `/api/v1/telemetry` `counters.dns_tcp_connections` through the existing 10 s poll, stored in the registry as one `ArcSwap` snapshot so a read never shows `active > peak`. The gauge read itself reports `max(peak, active)`: its two `Relaxed` loads can land between a connection's `fetch_add` and its `fetch_max`, and the raw `peak` is never above the true high-water mark, so the reported figure is a lower bound at least as tight as the atomic — never lower, never above the ceiling. Per connection: one semaphore permit (an `Arc` clone + acquire, released after the gauge decrement), one `Arc` clone for the gauge guard, three atomic RMWs (`fetch_add` + `fetch_max` on enter, `fetch_sub` on close); per message: one compare. Worst-case in-flight request buffers: 1024 × 16 KiB = 16 MiB — the request `Vec` only; the per-connection task, `TcpStream`, kernel socket buffers and hickory parse allocations sit on top of that. The key has no upper bound: `Semaphore::new` panics above `MAX_PERMITS`, same as `[http] max_connections`. **1024 and 16 KiB are initial safety bounds, not tuned values.** Next: 7-day device soak, then a final default from observed `peak` + operational headroom + the container fd budget — not a mechanical multiple. |
| F2 | material | recommendation | `fah-dns/src/udp.rs:75-80` | One `tokio::spawn` per datagram, no in-flight cap anywhere in `fah-dns` (no `Semaphore`). In-flight memory = arrival rate × walk time; under an upstream outage walk time is `worst_case_walk` (`ATTEMPT_LEGS` = 3 × timeout). Arithmetic only — measure before adding admission control. |
| F3 | minor | recommendation | `fah-dns/src/pipeline.rs:312` | `queries.first().cloned()` deep-copies the `Query` (hickory `Name`) once per query; `request` is never mutated in `handle`, so a borrow would do. One avoidable allocation for names past hickory's inline label capacity. `forward_alloc.rs` asserts adaptive = fallback, not an absolute count, so this is not caught. |
| F4 | minor | recommendation | `fah-dns/src/cache.rs:519,555,566,626,670,680,710,769`; `fah-rules/src/lifecycle/mod.rs` ×20; `fah-stats/src/stats.rs` ×14; `fah-dns/src/swr.rs:155` | ~43 `lock().unwrap()` on `std::sync` locks outside tests. A panic inside any critical section (none identified) poisons that lock and every later taker panics: a poisoned cache shard fails 1/16 of lookups per query task; a poisoned `aggregates` kills the event fan-out task (`main.rs:536`) on its next `record`, after which the events channel fills and `dropped_events` counts — stats freeze, nothing logs (see F11). `unwrap_or_else(PoisonError::into_inner)` needs no new dependency. Not a defect today. |
| F5 | minor | recommendation | `fah-rules/src/lifecycle/mod.rs:769,1326`; `fastadhunter/src/main.rs:879` | Three `spawn_blocking` join `.expect`s turn a panic in the blocking closure into a panic in the awaiting task: 769 (list validation parse) reaches an API task; 1326 (ruleset compile) reaches the scheduler task and the detached API refresh (`routes.rs:788`); 879 (`collect_memory`) reaches the perf sampler. A panic in the scheduler or sampler is never observed (F11). Parser and compile are property-tested; paths unexercised. |
| F6 | minor | recommendation | `fah-http/src/proxy.rs:395-467` | `judge()` allocates host, path, method; `emit()` then clones `ModelRequest`, `Verdict` and the policy `Arc` again. ~6 allocations per HTTP request for the event; whether `Judged` must outlive `emit` was not checked. |
| F7 | minor | recommendation | `fah-dns/src/udp.rs:94`, `tcp.rs:80` | Logging is synchronous stderr (`fah-logging/src/lib.rs:62`). Per-query DNS sites are `trace!`/`debug!`; the all-upstreams-down `warn!` is rate-limited by the alarm (`upstream/mod.rs:308`). The only per-event `warn!`s under repeated failure are a failed UDP `send_to` and a non-disconnect TCP error — one blocking write per event on a runtime thread. Unmeasured. |
| F8 | minor | recommendation | `fah-stats/src/stats.rs:199-207,354` | `save_snapshot` clones and `snapshot()` walks the aggregates under the same `std::sync::Mutex` `record()` takes; the fan-out task stalls for the clone and the pipeline sheds to `dropped_events` rather than blocking. Bounded and counted; no action. |
| F9 | minor | verified, no action | `fah-dns/src/upstream/encrypted.rs:48-50` | The comment says the provider "owns the JoinSet the exchanges' background I/O tasks spawn into". Checked in `hickory-net` 0.26.1 (`src/runtime.rs:139-148`): `TokioRuntimeProvider(TokioHandle { join_set: Arc<Mutex<JoinSet<()>>> })`; `spawn_bg` spawns into the set and reaps finished tasks (`:221`); clones share the `Arc`, so the last clone dropping aborts the I/O tasks. The comment is accurate. The `std::sync::Mutex` inside `spawn_bg` is taken per connect, not per query. |
| F10 | material | **fixed 2026-09-11** | `fastadhunter/src/main.rs` `Engine::shutdown`, `STATS_FLUSH_TIMEOUT` | Was: a clean stop never flushed stats — `Engine::shutdown` aborted both stats schedulers and nothing called `save_snapshot` or `flush_history` afterwards, so every SIGTERM/`container stop` lost up to `snapshot_interval_seconds` (300 s default) of aggregates, top-N and client registry. Now: `shutdown` is `async`, holds the `Stats` `Arc`, and after aborting the schedulers awaits `save_snapshot` then `flush_history` under a 5 s timeout (warn on expiry). `flush_history` is idempotent across ticks (unit-tested), so the extra call is safe. Still open: no process-level teardown test (item 3); events left in the fan-out channel at abort are dropped. |
| F11 | minor | recommendation | `fastadhunter/src/main.rs:256-258,293,530-545,596` | Long-lived task death is unobserved. `Engine::tasks` holds the rules scheduler, both stats schedulers, the event fan-out, the perf sampler, the SWR workers and the cache cleanup; the run loop `select!`s only on the shutdown signal and `dns.fatal()`, and the handles are aborted at shutdown, never polled. Tokio swallows a task panic into a `JoinError` nobody reads. Reachable panic sites: F5's join `expect`s and F4's poison unwraps. Consequence: the resolver keeps answering while refreshes, stats or SWR silently stop. A `JoinSet` in the `select!`, or `is_finished()` on the perf tick, would surface it. |
| F12 | minor | note | `fastadhunter/src/main.rs:243,254` | The runtime is dropped at the end of `main` with no `shutdown_timeout`. Tokio 1.53 `Runtime::drop` waits for blocking-pool tasks that are running, so a stop that lands inside a `spawn_blocking` compile (`lifecycle/mod.rs:1268`) or validation parse (`:764`) waits for it to finish. Bounded by compile time; relevant only to "a container that will not stop cleanly" in item 3. |

## Checked and clean

Recorded so the next pass can skip them.

- Per-query path allocates: datagram `to_vec`, task, hickory parse, `domain_of`
  `String` (`qtype.rs:24`), `CacheKey` `Box<str>` (`cache.rs:81`), response
  `Vec<u8>`; the matcher walk (`matcher.rs:900`) allocates nothing;
  `policy_for` (`policy.rs:288`) is a linear scan over the configured
  assignments.
- Ruleset and policies reach the query path through `ArcSwap::load_full` only;
  `drop(active); drop(matcher)` before the upstream await (`pipeline.rs:351`)
  keeps a swapped-out ruleset from being pinned by a slow forward.
- Every bounded queue sheds and counts rather than blocking: events channel
  (`try_send` + `dropped_events`, `pipeline.rs:500`, `proxy.rs:464`), SWR
  queue (`swr.rs:113`), SSE broadcast (`events.rs:18` capacity 256, `Lagged`
  drops the subscriber at `:272`). The HTTP domain handoff (`server.rs:238`)
  awaits a full queue — backpressure on the accept loop by design, the permit
  still held.
- Cancellation: no timeout wraps `pipeline.handle`, so a UDP task is never
  cancelled mid-claim; if one were, a refresh claim leaks for one lease
  (`DEFAULT_REFRESH_CLAIM_LEASE` 5 s, `cache.rs:69`) and expires.
  `ExchangeConn::acquire` holds its mutex across `connect`; dropping the guard
  on cancel lets the next waiter reconnect.
- Stats state is capped (`top_n.rs:86` `HOURLY_TRACKED_DOMAINS` 256,
  `client_registry.rs:16` 4096); the encrypted upstream `request.clone()` per
  attempt is the hickory `DnsRequest` API, counted by `forward_alloc.rs`.
- The DNS listener loops (`udp.rs:57`, `tcp.rs:49`) back off through
  `RetryPolicy` on `recv`/`accept` errors and return `ListenerDied`, which
  `main.rs:258` observes and exits on — the one task death that *is* watched.
- `fah-api`'s login limiter `per_address` map is bounded by `max_tracked` with
  expiry eviction (`password.rs:84-116`); `fah-metrics` has no labelled
  families, so cardinality is fixed.
- `unwrap`/`expect` outside `#[cfg(test)]`: rules 33, stats 20, dns 13, binary
  11, api 3, config 1, others 0. ~43 are the poison unwraps in F4, 3 are the
  join `expect`s in F5, the rest are `try_from` capacity invariants in the
  matcher builders (`matcher.rs:464,562`, `url_matcher.rs:807-809`).
- `unsafe` is six sites, all in the binary (`allocator.rs:57`,
  `privilege.rs:30/53/80/89`, `process.rs:22`), plus test-only `GlobalAlloc`
  shims in three alloc-counting tests.
- Not one `TODO`, `FIXME`, `HACK` or `XXX` in the source; the one `XXX0YYY` is
  a POSIX-TZ literal in a `tz.rs` test.
- 22 integration test files under `crates/*/tests/`, four of them
  allocation-counting (`forward_alloc`, `url_lookup_alloc`,
  `dedup_alloc_bound`, `heap_cost`) and 12 criterion benches under
  `crates/*/benches/`. Root `tests/` and `benches/` are `.gitkeep`-only.
- Hard rule 7 ("no comments in Rust code") is enforced by the hook on new edits
  only: `main` carries ~5,100 `///` and ~1,900 `//` lines in `crates/`. Not a
  defect — recorded so nobody strips them as a "fix".

## Remaining TODOs

- F1 and F10 fixed on `main` (see their rows). F1's soak is open: after 7 days
  on the RB5009 read `counters.dns_tcp_connections.{peak,closed_oversize}`
  from `/api/v1/telemetry`, check the container fd budget, and set the final
  `tcp_max_connections` default; record corpus, workload and device in a
  `docs/code-review/` file. F2 is the remaining candidate, with measurement
  before any cap is chosen; F11 is record-only until then. F3–F9, F12 are
  record-only.
- Outside this file, needing an owner go: [CLAUDE.md](../../../CLAUDE.md)
  §Layout lists root `tests/` and `benches/` — both are `.gitkeep`-only;
  every test and bench lives under `crates/*/`.
  [project-state.md](../../project-state.md) still says `alloc-domains/http`
  awaits its soak verdict (`main` at `0a716ec`), but `git` shows it fully
  merged (0 commits ahead, `main` 23 ahead) and `main` at `baa2ecd`. Neither
  doc was edited in this pass.
- Revisit this inventory when Phase 3 closes, not before.
