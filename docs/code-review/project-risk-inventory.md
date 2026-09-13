# Project risk inventory — surveyed on `main` at `baa2ecd`, 2026-09-11; F1, F2 and F10 closed the same day, F11, F3 and F6 on 2026-09-12 (§Closed); F13 opened 2026-09-13 on the Phase 3 merge

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
Teardown is `Engine::shutdown`: http → dns → api → abort every long-lived task
(`Engine::tasks` holds all of them but the two stats schedulers, which sit in
`Engine::stats_schedulers` so shutdown can abort *and await* them before the
final stats flush); the DNS listener tasks are aborted in
`fah-dns/src/server.rs:107`. The runtime is then dropped at the end of
`main` — per-connection DNS-TCP and HTTP tasks end with it, HTTP domains drain
under `HTTP_DRAIN_TIMEOUT` (5 s). Process teardown has one test that can fail:
`tests/shutdown_e2e.rs` (unix only) sends SIGTERM to the spawned binary and
asserts a clean exit and the flushed snapshot; drain and task-death behaviour
are still untested. 11b (graceful shutdown of keep-alive connections) is still
open in
[alloc-domains-http-review.md](../phase2.6/alloc-domains-http-review.md).
One teardown finding below: runtime drop unbounded by a `shutdown_timeout`
(F12). The missing stats flush on a clean stop (F10) and unobserved
long-lived task death (F11) are closed.

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
**material** = worth a task before Phase 3 closes (none open — F1, F2 and F10
moved to §Closed); **minor** = record only.
Class: **defect** = confirmed against source; **recommendation** = a
judgement, unmeasured.

| # | Severity | Class | Where | Finding |
| --- | --- | --- | --- | --- |
| F4 | minor | recommendation | `fah-dns/src/cache.rs:519,555,566,626,670,680,710,769`; `fah-rules/src/lifecycle/mod.rs` ×20; `fah-stats/src/stats.rs` ×14; `fah-dns/src/swr.rs:155` | ~43 `lock().unwrap()` on `std::sync` locks outside tests. A panic inside any critical section (none identified) poisons that lock and every later taker panics: a poisoned cache shard fails 1/16 of lookups per query task; a poisoned `aggregates` kills the event fan-out task (`main.rs:536`) on its next `record`, after which the events channel fills and `dropped_events` counts — stats freeze; the supervisor logs the death and counts it. `unwrap_or_else(PoisonError::into_inner)` needs no new dependency. Not a defect today. |
| F5 | minor | recommendation | `fah-rules/src/lifecycle/mod.rs:769,1326`; `fastadhunter/src/main.rs:879` | Three `spawn_blocking` join `.expect`s turn a panic in the blocking closure into a panic in the awaiting task: 769 (list validation parse) reaches an API task; 1326 (ruleset compile) reaches the scheduler task and the detached API refresh (`routes.rs:788`); 879 (`collect_memory`) reaches the perf sampler. A panic in the scheduler or sampler is logged and counted by the supervisor but still ends that task. Parser and compile are property-tested; paths unexercised. |
| F7 | minor | recommendation | `fah-dns/src/udp.rs:94`, `tcp.rs:80` | Logging is synchronous stderr (`fah-logging/src/lib.rs:62`). Per-query DNS sites are `trace!`/`debug!`; the all-upstreams-down `warn!` is rate-limited by the alarm (`upstream/mod.rs:308`). The only per-event `warn!`s under repeated failure are a failed UDP `send_to` and a non-disconnect TCP error — one blocking write per event on a runtime thread. Unmeasured. |
| F8 | minor | recommendation | `fah-stats/src/stats.rs:199-207,354` | `save_snapshot` clones and `snapshot()` walks the aggregates under the same `std::sync::Mutex` `record()` takes; the fan-out task stalls for the clone and the pipeline sheds to `dropped_events` rather than blocking. Bounded and counted; no action. |
| F9 | minor | verified, no action | `fah-dns/src/upstream/encrypted.rs:48-50` | The comment says the provider "owns the JoinSet the exchanges' background I/O tasks spawn into". Checked in `hickory-net` 0.26.1 (`src/runtime.rs:139-148`): `TokioRuntimeProvider(TokioHandle { join_set: Arc<Mutex<JoinSet<()>>> })`; `spawn_bg` spawns into the set and reaps finished tasks (`:221`); clones share the `Arc`, so the last clone dropping aborts the I/O tasks. The comment is accurate. The `std::sync::Mutex` inside `spawn_bg` is taken per connect, not per query. |
| F12 | minor | note | `fastadhunter/src/main.rs:243,254` | The runtime is dropped at the end of `main` with no `shutdown_timeout`. Tokio 1.53 `Runtime::drop` waits for blocking-pool tasks that are running, so a stop that lands inside a `spawn_blocking` compile (`lifecycle/mod.rs:1268`) or validation parse (`:764`) waits for it to finish. Bounded by compile time; relevant only to "a container that will not stop cleanly" in item 3. |
| F13 | minor | recommendation | `fah-dns/src/dot.rs:63,152`; `fah-dns/src/tcp.rs:138` | DoT reuses `handle_connection` but passes `None` for the gauge, deliberately: mixing it into `dns_tcp_connections` would corrupt the figure that sizes `[dns] tcp_max_connections`. The consequence is that DoT has no telemetry at all — no `active`, no `peak`, no oversize count — and its 64-connection cap is compiled in, so a saturated DoT listener is invisible on `/telemetry`. Not a defect; a `dns_dot_connections` block on the same shape would close it. Recorded 2026-09-13 on the Phase 3 merge; scoped to `phase3-06` merged into `main`, since DoT does not exist on `main` alone. |

## Closed

Retired from the findings table by the close-out audit of 2026-09-11 on
`main` following `b0b091e`, and by the later passes that name their own date
below. Kept here so the next pass knows what was verified and against what.

- **F6 — `judge()`/`emit()` allocations on the HTTP request path** (was minor,
  recommendation). Closed by `3287418` (H1-H3/D1), 2026-09-12, after the
  survey: `emit` takes `Judged` by value, so `ModelRequest`, `Verdict` and the
  policy `Arc` are moved rather than cloned, and `judge` no longer builds an
  authority `String` — it passes the host and an `Option<u16>` port to
  `absolute_url`. The remaining allocations are the event's own `host`, `path`
  and `method`, which the event owns. Guarded by
  `fah-http/tests/proxy_alloc.rs`. Re-confirmed on the Phase 3 merge,
  2026-09-13: the branch's shared `judge`/`emit` free functions carry the
  by-value form, so the merge did not put the clones back.

- **F1 — DNS-over-TCP unbounded** (was material). `[dns] tcp_max_connections`
  (boot, default 1024) sizes a semaphore taken before `accept`; a 2-byte
  length prefix above `MAX_MESSAGE_LEN` (16 KiB) closes the connection without
  allocating. `counters.dns_tcp_connections {active, peak, closed_oversize}`
  on `/api/v1/telemetry`. Per connection one permit, one gauge guard, three
  atomic RMWs; per message one compare; worst case 1024 × 16 KiB = 16 MiB of
  request buffers. Verified: `fah-dns/src/tcp.rs` tests (the ceiling holds the
  next accept, an oversize prefix closes and counts, an at-bound prefix is
  read); `fah-dns/tests/server_integration.rs`
  `the_configured_tcp_and_udp_ceilings_reach_the_listeners` (an explicit
  `tcp_max_connections = 1` holds a second connection until the first closes,
  so the config value reaches the listener); `fah-config` rejects 0;
  `fah-metrics` round-trip; `fah-api` shape. Recorded, not a defect: the
  semaphore panics above `MAX_PERMITS`, no upper bound, same as
  `[http] max_connections`. The 7-day soak that picks the final default is a
  tuning follow-up (§Remaining TODOs), not a risk.
- **F2 — UDP in-flight unbounded** (was material). Measured in
  [phase2.6/f2-udp-inflight.md](phase2.6/f2-udp-inflight.md): a full black
  hole walks 3.23 s at four UDP upstreams and `timeout_ms = 800`, ~8.1 KiB heap
  per in-flight query, so 26 KiB per qps of arrival; the household's 30-day
  peak of 10.5 qps is 0.3 MiB. `[dns] udp_max_inflight` (boot, default 0 = no
  cap and no admission accounting, so `active` and `peak` stay 0) bounds an
  atomic in-flight counter checked before the datagram is copied; past the
  ceiling the datagram is dropped unanswered and counted in `shed`. At 0: two
  branches per datagram, no atomics; enabled: one CAS loop plus one
  `fetch_sub`, no lock, no permit, no extra `Arc` clone. Owner decision: the
  household stays at 0; larger deployments set it as a memory guardrail.
  Verified: `fah-dns/src/udp.rs` tests (sheds past the limit, a ceiling of
  two, a released slot re-admits, zero never sheds);
  `fah-dns/tests/server_integration.rs` (an explicit `udp_max_inflight = 1`
  sheds the second datagram while the first is in flight); `fah-metrics`
  round-trip; `fah-api` shape.
- **F10 — no stats flush on a clean stop** (was material). `Engine::shutdown`
  is async, holds the `Stats` `Arc`, aborts *and awaits* the two stats
  schedulers (no scheduler tick can start a write after the flush begins),
  then runs `save_snapshot` and `flush_history` — all under one 5 s
  `STATS_FLUSH_TIMEOUT`, warn on expiry. Runs on both exit paths (signal and
  listener death) before the runtime drops. Verified:
  `fastadhunter/tests/shutdown_e2e.rs` (unix only) boots the binary, resolves
  a name, waits for the query to reach `/api/v1/stats`, sends SIGTERM, and
  asserts a clean exit and that `/data/stats/snapshot.json` carries the query
  and the client — with `snapshot_interval_seconds = 300` the shutdown flush
  is the only write that can have done so. `flush_history` captures completed
  hours only, so a post-boot query inside the current hour is proven through
  the snapshot, not the rollups. Residual, not a risk: a `tokio::fs` write a
  scheduler had already dispatched when the abort landed can still finish
  after the await; for history that is a rollup line without its `\n`
  followed by the flush's duplicate, which the reader skips (one hour lost) —
  pre-existing, a microsecond window once per 300 s. Events still in the
  fan-out channel at abort are dropped.
- **F11 — long-lived task death unobserved** (was minor). `Engine::run`
  scans every supervised handle — `Engine::tasks` and
  `Engine::stats_schedulers`, each wrapped in `supervisor::Supervised` with a
  name — on a 10 s `TELEMETRY_POLL` tick in the run loop's `select!`, the one
  task that cannot die silently. A handle finished before shutdown is
  removed, awaited, classified (`panicked` with the payload, `returned`,
  `cancelled`), logged once at `error` with task name and cause, and counted
  in `counters.tasks_died` on `/api/v1/telemetry` (dashboard Engine card, red
  when non-zero). Report-only by owner decision: a restart would re-hit the
  poisoned lock or deterministic `expect` that killed it; an exit would take
  household DNS down, since RouterOS restarts nothing
  ([routeros-traps.md](../routeros-traps.md)). `/health` unchanged. Not
  covered: the API serve task. `is_finished()` on a tick was chosen over a
  `JoinSet` because the four library spawn functions return `JoinHandle`s a
  `JoinSet` cannot adopt; the alternative was an API change in three crates
  or one wrapper task per supervised task. Verified:
  `fastadhunter/src/supervisor.rs` tests (panic payload as `&str` and
  `String`, returned, aborted, a running task stays, reported once, empty
  set), `fah-metrics` counter test, dashboard Engine card test. The
  `Engine::run` wiring is not e2e-tested: no supervised task can be made to
  panic from outside the binary.

- **F3 — per-query `Query` clone** (was minor). `Pipeline::handle` borrows
  `request.queries.first()` instead of cloning it. `request` is a local owned
  for the whole future and both `response::blocked` and `resolve` take
  `&WireQuery`, so ownership and await-safety are unchanged. hickory `Name`
  inlines up to 32 bytes of label data, so the clone allocated only for
  longer names. Measured with `fah-dns/tests/forward_alloc.rs`
  `warm_pipeline_handles_allocate_a_steady_amount` (counting `GlobalAlloc`
  over mimalloc, 64 warm `handle` calls, x86 dev box, `main` at `0fb8dd0`).
  Two paths: blocked, and cache hit — the stub forwarder's empty `NOERROR`
  answer is negative-cached on the first warm call, so every measured
  `handle` after it is served from the cache and the forwarder never runs in
  a measured batch. Blocked heap-name 1472 → 1408, cache-hit heap-name
  1280 → 1216, one allocation per query; inline names unchanged at 960 and
  704. The test asserts no accumulation and, since the follow-up, a
  per-handle ceiling. Heap names still cost 7–8 more allocations per query
  than inline names; every one is attributed in
  [phase2.6/f3-name-alloc-attribution.md](phase2.6/f3-name-alloc-attribution.md),
  which also pre-sized `domain_of` (15 / 22 / 11 / 19 → 13 / 19 / 10 / 16
  per handle). The rest is hickory-internal or required by the response
  representation — record only.

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
- 23 integration test files under `crates/*/tests/`, four of them
  allocation-counting (`forward_alloc`, `url_lookup_alloc`,
  `dedup_alloc_bound`, `heap_cost`), one unix-only (`shutdown_e2e`), and 12
  criterion benches under `crates/*/benches/`. Root `tests/` and `benches/`
  are `.gitkeep`-only.
- Hard rule 7 ("no comments in Rust code") is enforced by the hook on new edits
  only: `main` carries ~5,100 `///` and ~1,900 `//` lines in `crates/`. Not a
  defect — recorded so nobody strips them as a "fix".

## Remaining TODOs

- F1 tuning follow-up (not a risk — the bound is in place, this picks its
  value): after 7 days on the RB5009 read
  `counters.dns_tcp_connections.{peak,closed_oversize}` from
  `/api/v1/telemetry`, check the container fd budget, and set the final
  `tcp_max_connections` default; record corpus, workload and device in a
  `docs/code-review/` file. F4–F9, F12 are record-only.
- Outside this file, needing an owner go: [CLAUDE.md](../../../CLAUDE.md)
  §Layout lists root `tests/` and `benches/` — both are `.gitkeep`-only;
  every test and bench lives under `crates/*/`.
  [project-state.md](../../project-state.md) still says `alloc-domains/http`
  awaits its soak verdict (`main` at `0a716ec`), but `git` shows it fully
  merged (0 commits ahead, `main` 23 ahead) and `main` at `baa2ecd`. Neither
  doc was edited in this pass.
- Revisit this inventory when Phase 3 closes, not before.
