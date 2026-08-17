# p2-09 — API telemetry consolidation

## Summary

One `GET /api/v1/telemetry` now serves the whole engine state as JSON, so a
dashboard makes one call instead of polling `/api/v1/cache` +
`/api/v1/debug/memory` + `/metrics` and parsing the last as Prometheus text.
The rule count, per-stage latency totals and per-upstream counters had no JSON
home anywhere — 23 of `/metrics`' 33 families did not.

`GET /api/v1/queries` is removed, and with it the whole query log — a grep
across `crates/`, `tui-monitor/`, `requests/` and `scripts/` found no consumer
of either tier. Gates green: **776 tests, 40 binaries**.

This task replaced the former p2-09 ("Query Log Reader"), which would have made
the `/data` segments searchable *through* that endpoint. See
[the task file](../../../plan/wip/phase2/p2-09-api-telemetry-consolidation.md) for
why the reversal holds.

## Decisions

- **Metrics are placed by who produces them.** Application + kernel figures →
  `/telemetry` (stable contract); allocator internals → `/debug/*` (no promise).
  Expressed in types: `DebugMemoryResponse` embeds `MemoryResponse`, so the two
  endpoints cannot disagree on a shared field, and a test asserts the difference
  is *exactly* the two `allocator_committed_*` fields.
- **`EngineTelemetry` lives in `fah-model` and is the wire shape itself.** A DTO
  layer earns its place when it transforms; here it would have been the identity
  function across eight structs plus a second allocation per request.
  `MemoryComponents` keeps its DTO because that one renames, flattens and
  derives.
- **Latency ships `count` + `sum_seconds`, never an average.** A lifetime mean
  flattens within hours of uptime. Deltaing two reads gives the interval mean;
  percentiles stay with `/history/perf`, which already windows them.
- **`process.uptime_seconds` is mandatory.** Every counter is process-lifetime,
  so without it a client cannot distinguish a restart from a counter going
  backwards. p2-11 recorded five container restarts in one soak window.
- **One gathering site for memory** (`MemorySnapshot::collect`), shared with
  `/debug/memory`; `TelemetrySnapshot` wraps it and adds the engine read.
  Bounded skew, not atomicity — the counters are independent atomics and RSS is
  a separate syscall — but it removes skew spread across three handlers plus a
  poll interval, which `residual = rss − Σcomponents` cannot tolerate.
- **Kernel figures and allocator figures are separate types behind separate
  port methods.** `ProcessStats` (getrusage) vs `AllocatorStats` (mimalloc), so
  an allocator that reports nothing nulls `allocator_committed_*` and only
  those. One shared `Option` made the stable contract unkeepable.

## Review findings, fixed in a second pass

Everything below was found by reviewing the branch as an outside PR, then
fixed. `requests/` and `project-state.md` were the real damage: the code was
consistent, its integration surface was not.

| Severity | Finding | Fix |
| -------- | ------- | --- |
| Blocking | **`requests/` was never updated.** 14 of `stats.http`'s 15 requests hit the deleted `/queries`; `settings.http` POSTed a `[query_log]` patch that now 422s; `README.md` documented the ring and `retention_max_mb`; `benchmark.ps1` read p99 out of `/queries`. And no `telemetry.http` existed at all, though the README promises a file per API.md endpoint | `stats.http` rewritten; new `telemetry.http`; `settings.http` case retargeted at the `deny_unknown_fields` 422; README table + trap rewritten; `benchmark.ps1` points at `latency.*` and `/history/perf` |
| Blocking | **The producer boundary was asserted in the wire types but not in the type behind them.** `process_peak_rss`, `major_page_faults`, `minor_page_faults` all read `memory.allocator.map(…)`, and `peak_rss`/`page_faults` came out of `mi_process_info` — so swapping the allocator, the exact scenario the split exists for, would null three fields `/telemetry` promises | `fah_model::ProcessStats` split out, `TelemetrySource::process()` added, `crates/fastadhunter/src/process.rs` reads all three from one `getrusage`. Two tests pin it, one with `allocator()` returning `None` |
| Blocking | **`/debug/memory` paid for an engine snapshot it discarded** — `TelemetrySnapshot::collect` cloned `Vec<UpstreamSample>` and read ~20 atomics for a response using none of it. The same objection `engine_telemetry()` raises against `snapshot()` | `MemorySnapshot` / `TelemetrySnapshot` split |
| Blocking | **`project-state.md` was false in every particular** — p2-09 listed `BLOCKED`, §Blocker describing the ring and `/queries` as live | Rewritten: blocker gone, deploy ordering promoted to its own section |
| Blocking | **A clock-dependent `assert_eq!` in `e2e.rs`** compared `/telemetry.cache` to `/cache` across two HTTP calls. `fresh`/`stale`/`expired` are a walk against `now` with `BLOCK_TTL = 10 s`, so a TTL boundary landing between them fails the test | Compares the field set plus `capacity`; the value-for-value version already exists in `api.rs` against a frozen fake |
| Blocking | `history.http` had `resolution=day` changed to `hour` under a comment saying "one point per UTC day" — an unrelated stray edit that deleted the only `resolution=day` coverage | Restored |
| Blocking | **`/metrics` did not reconcile with itself.** `process_resident_memory_bytes` re-read `/proc/self/status` live while every other memory gauge came from the pushed snapshot, so `resident − Σcomponents ≠ residual` by up to one 10 s poll — the exact skew `MemoryBreakdown` exists to forbid | Renders `memory.rss` from the same snapshot. A test asserts the three reconcile. `fah-metrics` now reads no file at all, which also drops its `fah-common` dependency (and with it tokio + socket2, on a crate whose manifest says "No tokio") |
| Blocking | **`fastadhunter_ruleset_heap_bytes` duplicated `fastadhunter_memory_component_bytes{component="ruleset"}`** — the same `Matcher::heap_bytes()`. Worse than cosmetic: producing it called `heap_bytes()` a **second** time per poll, and that call sat *outside* the `spawn_blocking` the surrounding comment says exists to keep this walk (495 µs–4.86 ms on-device) off a DNS worker | Gauge and `RulesetSnapshot::heap_bytes` deleted; the binary reads the size once, inside the blocking pass. **Breaking for anyone scraping that series** — see TODOs |
| Minor | `Protocol` derived `Default = Udp` with no caller — an *observed* transport defaulting to the unencrypted one | Derive removed |
| Minor | `UpstreamSample::protocol` became a closed enum, so a perf row naming a transport this build does not know is dropped **whole** by `HistoryReader` — taking its RSS, latency and cache series with it. The file format is already lenient about unknown *keys* | `Protocol::Unknown` + a hand-written `Deserialize` (`#[serde(other)]` covers only tagged enums). Uses `visit_str`, so it works under `from_value` too |
| Minor | Stale comments naming the deleted subsystem in `matcher.rs`, `url_matcher.rs`, `filtering.rs`, `http_e2e.rs`, `api.rs`, `fah-stats/Cargo.toml`, `CONFIGURATION.md`, `memory.rs` (`retention_max_mb` exists nowhere now), `measurement-traps.md` | Removed |
| Minor | `benchmark.ps1` claimed `fastadhunter_ruleset_compile_duration_seconds` is hardcoded to zero. It has been real since the lifecycle started reporting it | Corrected |
| Minor | **Pre-existing flake** (p2-07, surfaced under parallel load): `overlapping_lists_report_compiled_rules_net_of_duplicates` polled for `compiled_rules == 3`, which list `b` satisfies *alone* — 3 rules, 0 duplicates — then asserted `duplicates_removed == 2` outside the poll | Polls on both conditions |

## Bugs found

| Where | Defect |
| ----- | ------ |
| This task's own plan | Claimed the websocket reads the query-log ring. It does not — `events.rs` renders events pushed through `EventHub`, and `Stats::queries()` was the ring's only reader. That is what made removing the whole log correct rather than just the endpoint. |
| `fah-stats` | `query_log_overflow_dropped()` was documented "for fah-metrics" and called by nothing but a test — a counter nobody read, guarding a batch nobody flushed anywhere readable. |
| `fah-stats` | `record()` and `record_http()` took a client-registry name lookup **plus a `String` clone** per event, purely to stamp the log entry. Gone — the websocket already resolved the name at publish time, behind `EventHub::has_subscribers()`. The DNS path now pays it only while a dashboard is attached, which on the appliance is ~0 h/day. The one real hot-path win in this task. |
| `fah-common` / `fah-api` | `/proc/self/status` was hand-parsed **twice** (`fah_metrics::process`, `fah_api::rss`), with different failure signalling (`0` vs `Option`). Folded into `fah_common::process::resident_bytes`, keeping `Option` — a fabricated `0` charts as a real measurement. Deleted a `match … 0 => None` workaround in the binary. |
| `fah-metrics` | `UpstreamSnapshot` was field-identical to `fah_model::UpstreamSample` once `protocol` was typed. Deleted; the pool status, the registry and every `PerfSample` are now one type with no remap between them. |
| `fah-dns` → API | `protocol` was a `&'static str` / `String` at four hops. Now `fah_model::Protocol`. Wire spelling unchanged, so persisted rows still deserialize (asserted). |

## Measurements

Read-path only; nothing on the hot path changed, so no bench was run.

| Axis | Before (3 pollers) | After (1 poller) |
| ---- | ------------------ | ---------------- |
| `cache.stats()` bounded walks | 2 (`/cache`, and again inside `debug_memory`) | 1 |
| TLS handshakes / round trips | 3 | 1 |
| Body transferred | ~3 KB Prometheus text (a subset of the data) | ~1.5–2 KB JSON (all of it) |
| Heap allocations per telemetry request | — | no bucket vectors: `engine_telemetry()` reads `count`/`sum_seconds` directly rather than via `snapshot()`, which allocates 5 × `Vec<u64>` |
| Allocations per `/debug/memory` request | — | no upstream vector: it gathers `MemorySnapshot`, not the engine |
| `getrusage` calls per telemetry poll | 2 (`mi_process_info` internally, then one for minor faults) | 1 |
| `Matcher::heap_bytes()` walks per telemetry poll | 2, one of them on a runtime worker | 1, inside the `spawn_blocking` |

Steady-state memory is unchanged: responses are built per request and dropped.

Per DNS query, off the measured path: one client-registry lookup and one
`String` clone removed while no dashboard is connected (see Bugs found).

## Files changed

| File | Change |
| ---- | ------ |
| `crates/fah-model/src/engine.rs` | **new** — `EngineTelemetry` + counters/latency/ruleset; `Serialize` + `Deserialize`, with paired `as_seconds`/`from_seconds` and `as_micros`/`from_micros` |
| `crates/fah-model/src/protocol.rs` | **new** — `Protocol { Udp, Dot, Doh, Unknown }`, no `Default`, hand-written `Deserialize` |
| `crates/fah-common/src/process.rs` | **new** — the workspace's one RSS reader |
| `crates/fastadhunter/src/process.rs` | **new** — the kernel figures, one `getrusage`, independent of the allocator |
| `crates/fah-model/src/memory.rs` | `ProcessStats` split out of `AllocatorStats`; `MemoryBreakdown` carries both as separate `Option`s |
| `requests/telemetry.http` | **new** — the endpoint, the producer boundary, and how to read a cumulative counter |
| `crates/fah-metrics/src/upstream.rs`, `src/process.rs`, `crates/fah-api/src/rss.rs` | **deleted** |
| `crates/fah-stats/src/query_log/` | **deleted** — ring, segment writer, filters, page types |
| `crates/fah-stats/src/stats.rs` | `log`, `pending_log`, `pending_dropped`, `segment`, `flush_query_log`, `requeue`, the flush scheduler and `query_log_overflow_dropped` removed |
| `crates/fah-config/src/schema/query_log.rs` | **deleted**, with the `[query_log]` section, its env-var arms and the default TOML block |
| `crates/fah-metrics/src/registry.rs` | `Metrics::engine_telemetry()` |
| `crates/fah-metrics/src/encode.rs`, `Cargo.toml` | RSS gauge off the snapshot; kernel/allocator blocks split; `fah-common` dependency dropped |
| `crates/fah-api/src/telemetry.rs` | **new** — `TelemetrySnapshot::collect()` + `TelemetryResponse` |
| `crates/fah-api/src/wire.rs` | `MemoryResponse` / `DebugMemoryResponse` split |
| `crates/fah-api/src/routes.rs`, `ports.rs`, `lib.rs` | route added, `/queries` plumbing removed |
| `crates/fastadhunter/src/adapters.rs`, `src/main.rs` | `engine()` impl; upstream remap deleted |
| `API.md`, `CONFIGURATION.md`, `README.md` | §Telemetry; `/queries` marked removed; the WS section now carries the event shape it used to reference |
| `requests/stats.http`, `settings.http`, `README.md`, `benchmark.ps1`, `cache.http`, `history.http` | dead `/queries` requests removed, `resolution=day` restored |
| `docs/project-state.md`, `docs/measurement-traps.md` | rewritten for a world without the query log |

## Remaining TODOs

- **Deploy is order-dependent.** `[query_log]` must be deleted from
  `/config/fastadhunter.toml` **before** the new binary starts — the root
  `Config` is `deny_unknown_fields`, so an unknown section is a boot failure,
  not a warning. Nothing migrates the file. A stale `/data/query_log/` is inert
  and can be deleted to reclaim disk.
- **On-device check not yet run.** `GET /api/v1/telemetry` against the container,
  confirming every chartable field is populated and agrees with the equivalent
  `/metrics` family read at the same instant.
- **The p2-07 residual steps at this deploy.** The ring's bytes leave the
  `stats` component and reappear in the residual. Re-baseline; a step is not a
  slope ([`measurement-traps.md`](../../measurement-traps.md) §Disk and retention).
- Any dashboard client can now collapse three pollers into one and drop its
  Prometheus text parsing. Not done here.
- **`/metrics` loses one series: `fastadhunter_ruleset_heap_bytes`.** The only
  removal in this task. Replacement is
  `fastadhunter_memory_component_bytes{component="ruleset"}`, identical value,
  already exported — so a scraper needs a query edit, not new collection. No
  in-repo consumer; nothing on the RB5009 scrapes `/metrics` today.
