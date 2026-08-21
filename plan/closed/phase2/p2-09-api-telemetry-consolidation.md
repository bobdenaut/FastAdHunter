# P2-09 — API telemetry consolidation (one JSON surface for a dashboard)

**Phase:** 2 · **Depends on:** p1-09 (`/metrics`), p2-07 (memory breakdown),
p2-04 (HTTP counters) · **Model:** Opus

> **This task replaces the previous p2-09 ("Query Log Reader") entirely.** That
> task existed to make the persisted query-log segments searchable through
> `GET /api/v1/queries`. This one **removes that endpoint**. The reversal is
> deliberate: `[query_log] enabled = false` in the deployment, so nothing is
> being written to the segments, and the live view a dashboard actually uses is
> the websocket. Reinstating the reader means reinstating both — the old task
> file is in git history if that day comes.

## Goal

Give a dashboard — web, TUI, or a scraper feeding an external store — **the
whole engine state in one authenticated JSON request**.

Today a client rendering one screen must poll `/api/v1/cache`,
`/api/v1/debug/memory` **and** `/metrics`, then parse the third as Prometheus
**text**, because three things it needs exist in no JSON response anywhere: the
compiled rule count, the per-stage latency figures, and the per-upstream
counters.

**Nothing in this task touches the hot path.** No DNS query and no HTTP request
executes any of this code; no new runtime state, lock or per-request allocation
is introduced on any request path.

## Why this exists

The read surface grew endpoint-by-endpoint and no longer answers the one
question a dashboard asks. The investigation that produced this task found the
problem is *not* mostly duplication:

| Finding | Detail |
| --- | --- |
| `/api/v1/stats` and the websocket `stats` push are **the same payload** | [`events.rs:124`](../../../crates/fah-api/src/events.rs) and [`routes.rs:194`](../../../crates/fah-api/src/routes.rs) both call `stats.overview(now)` → `StatsResponse`. The one genuine duplication. |
| `/api/v1/cache` and `/api/v1/debug/memory` **do not overlap at all** | Disjoint field sets, two separate polls. |
| `/metrics` is **~70 % unique** | Of 33 metric families: 8 duplicate `/debug/memory` (deliberately — one `MemoryBreakdown` feeds history, metrics and the API so a component cannot be added to one path and forgotten on another, [`memory.rs:112`](../../../crates/fah-model/src/memory.rs)), 2 duplicate `/cache`, and **23 have no JSON equivalent anywhere**. |

So the problem is the **absence of one aggregate endpoint**, not redundancy.

## Settled design — do not re-litigate

Argued to a conclusion with the user. Each row has its reason; change one only
with a reason that beats it.

| Decision | Reason |
| --- | --- |
| **Metrics are placed by who produces them.** Application + kernel/process → `/api/v1/telemetry` (stable contract). Implementation-specific runtime → `/api/v1/debug/*` (free to change). | Gives a dashboard something to code against while leaving allocator internals replaceable. |
| Only `allocator_committed_bytes` / `allocator_committed_peak_bytes` are debug-side | They read near zero under a different allocator ([`memory.rs:43`](../../../crates/fah-model/src/memory.rs)). The `getrusage` figures (`process_peak_rss`, major/minor page faults) survive an allocator swap **and are already persisted in `PerfSample` / served by `/history/perf`** — exiling them would give the history series a field the live endpoint lacks. |
| **`latency` is `count` + `sum_seconds`, never a pre-divided average** | Two polls then yield an *interval* average; a lifetime average flattens within hours of uptime. Raw buckets stay out — `/history/perf` already serves windowed percentiles. |
| **`process` is an object**, not top-level scalars | Identity can grow without cluttering the root. |
| **`uptime_seconds` is mandatory** | Every counter is process-lifetime. Without it a client charting rates cannot tell a restart from a bug — a `LAG()`-style delta across a restart yields a large negative. p2-11 saw **five container restarts** in one soak window. |
| **`ruleset.heap_bytes` is dropped** | Same `Matcher::heap_bytes()` as `memory.ruleset_bytes` ([`routes.rs:170`](../../../crates/fah-api/src/routes.rs)), differing only in freshness. Memory sizes live in `memory`. (`/metrics` already carries this same duplicate as `fastadhunter_ruleset_heap_bytes` vs `fastadhunter_memory_component_bytes{component="ruleset"}` — pre-existing, not introduced here.) |
| **Derivation is snapshot → response, never response → response** | Endpoints must not be built from each other, or they become coupled. Both `/cache` and `/telemetry` render `CacheStatsResponse::from(port.stats())` *independently*. |
| Both endpoints **share the wire type** | That is what makes `/telemetry.cache` and `/cache` structurally unable to drift. Two parallel types would reintroduce the divergence this task removes. |
| **One gathering site**, `TelemetrySnapshot::collect()` | Not spread across the handler. It lives in `fah-api` — the only L3 crate holding all the ports; `fah-metrics` cannot see the cache or stats ports (siblings never import each other). |
| `collect()` buys **bounded skew, not atomicity** | The counters are independent atomics read one at a time and RSS is a separate syscall, so a simultaneous read is not achievable. What it removes is skew spread across three handlers *plus* a poll interval — which is what [`MemoryBreakdown`'s contract](../../../crates/fah-model/src/memory.rs) demands, since `residual = rss − Σcomponents` and any skew lands in it as noise. |
| `/metrics` **stays** | Rendered from the same registry, costs one function. It remains the Prometheus surface; `/telemetry` is the JSON one. |
| `/api/v1/stats` **stays** | A client needs one snapshot before the websocket's first 2 s tick, so the HTTP form is not dead weight even though the payload matches the push. |
| All three `/history/*` **stay** | `/history/top` is **not** redundant with the websocket top-N: the websocket is live 24 h, `/history/top` is per-day historical — a different question. |

### Compatibility contract

To be stated in API.md and in the module doc:

> `/api/v1/telemetry` is intended to remain backward-compatible across
> releases. New fields may be added; existing fields must not change meaning or
> units. Implementation-specific diagnostics that may change or disappear
> belong under `/api/v1/debug/*`, which carries no such guarantee.

## Scope

### 1. `GET /api/v1/telemetry`

```jsonc
{
  "process": { "version": "0.2.10", "uptime_seconds": 184920 },
  "ruleset": { "rules": 1043886, "duplicates_removed": 41207,
               "compile_duration_seconds": 7.412 },
  "counters": {
    "dns":  { "pass": 812044, "allow": 1201, "block": 96318,
              "cache_hits": 640119, "cache_misses": 269446, "cache_stale": 3187 },
    "http": { "pass": 4412, "allow": 0, "block": 918, "response_bytes": 148223904 },
    "events_dropped": 0,
    "swr": { "enqueued": 12044, "deduplicated": 3311, "dropped": 0,
             "completed": 8702, "failed": 31 },
    "cache_cleanup": { "runs": 308, "entries_removed": 44120,
                       "bytes_freed": 9871232, "last_duration_micros": 1842 }
  },
  "latency": {
    "dns":  { "block":     { "count": 96318,  "sum_seconds": 2.114 },
              "cache_hit": { "count": 640119, "sum_seconds": 18.907 },
              "forward":   { "count": 269446, "sum_seconds": 6021.338 } },
    "http": { "block":   { "count": 918,  "sum_seconds": 0.031 },
              "forward": { "count": 4412, "sum_seconds": 12.884 } }
  },
  "upstreams": [ { "address": "1.1.1.1:853", "protocol": "dot", "attempts": 201883,
                   "failures": 12, "consecutive_failures": 0, "tls_handshakes": 41 } ],
  "cache":  { /* verbatim GET /api/v1/cache */ },
  "memory": { /* GET /api/v1/debug/memory minus the two allocator_committed_* */ }
}
```

**JSON loses Prometheus's `# TYPE`.** `cache_cleanup.last_duration_micros` is a
last-value gauge and must **not** be deltaed
([`snapshot.rs:70`](../../../crates/fah-metrics/src/snapshot.rs)); a client has
no other way to tell it from a total, so API.md must say so.

**Do not build a full `MetricsSnapshot` for the latency block.**
`StageHistogram` carries `cumulative: Vec<u64>` — 5 stages × 11 buckets = 5 heap
allocations per request, none of which `/telemetry` reads. Read `count` and
`sum_seconds` off the histograms directly.

Reuse, do not redefine:

- [`ports.rs`](../../../crates/fah-api/src/ports.rs) — `CacheStats`,
  `StatsSource::heap()`, `TelemetrySource::allocator()`.
- [`wire.rs`](../../../crates/fah-api/src/wire.rs) — `CacheStatsResponse`,
  `MemoryComponentsResponse::of()`.
- `crates/fah-metrics/src/` — `SwrSnapshot`, `CleanupSnapshot`,
  `RulesetSnapshot`; `fah_model::UpstreamSample` already has exactly the
  upstream fields this serves.

`fah-api` **cannot import `fah-metrics`** (L3 siblings). The engine figures
therefore reach it either as a new `TelemetrySource` method returning an L1
(`fah-model`) DTO, or as port-local DTOs the binary translates into — the same
crossing `CacheStats` already makes. Prefer the L1 DTO: it removes a whole
translation layer, and `MemoryBreakdown` is the precedent for a shape three
crates share.

### 2. Fold the duplicate RSS reader

`fah_metrics::process::resident_memory_bytes()` and
`fah_api::rss::process_rss()` both hand-parse `/proc/self/status` for `VmRSS` —
two implementations of the same four lines, predating this task (engineering
principle 4). `collect()` touches this path, so collapse it to one in
`fah-common` (L1, already the home of `listen`/`resolve`/`egress`; **not**
`fah-model`, which hard rule 2 keeps free of I/O).

**Keep the `Option` return, not the `0`** — a fabricated zero charts as a real
measurement. This also deletes the `match … 0 => None` workaround at
[`main.rs:661`](../../../crates/fastadhunter/src/main.rs).

### 3. Remove `GET /api/v1/queries`

The websocket carries live queries and `[query_log] enabled = false` in
deployment. Removes the handler, `parse_query_params`, and the
`QueryPageResponse` / `QueryLogRequest` / `QueryLogPage` / `VerdictFilter`
plumbing with their tests, across
[`routes.rs`](../../../crates/fah-api/src/routes.rs),
[`wire.rs`](../../../crates/fah-api/src/wire.rs),
[`ports.rs`](../../../crates/fah-api/src/ports.rs),
[`adapters.rs`](../../../crates/fastadhunter/src/adapters.rs) and the tests.

### 3b. Remove the query log entirely

Verified first: **nothing consumed it.** The websocket does not read the ring —
`events.rs` renders events pushed through `EventHub` — so `Stats::queries()` was
the ring's only reader, the `/data` segments never had one at all, and
`query_log_overflow_dropped()` was called by nothing but a test. A grep across
`crates/`, `tui-monitor/`, `requests/` and `scripts/` found no CLI, no dashboard
use, and no planned-but-unimplemented feature.

So the whole subsystem goes: `crates/fah-stats/src/query_log/`, `Stats::{log,
pending_log, pending_dropped, segment, flush_query_log, query_log_overflow_
dropped}`, the flush scheduler, `QueryLogEntry`, `StatsHeap::{ring,
pending_log}`, the two `query_log_*_bytes` fields on `/debug/memory` and
`/metrics`, and the `[query_log]` config section.

Two consequences to carry:

- **`[query_log]` in a deployed `fastadhunter.toml` becomes a boot failure**,
  because the root `Config` is `deny_unknown_fields`. Config edit first, deploy
  second.
- **`record()` no longer resolves a client name per event** — that lookup
  existed only to stamp the log entry. The websocket resolves it at publish
  time instead.

Per-query history is now `WS /api/v1/events` plus whatever consumes it.

### 4. Documentation (propose, then wait)

Per the working agreement no `.md` is edited without an explicit yes. After the
code is green, propose:

- **API.md** — new §Telemetry carrying the producer boundary, the compatibility
  contract and the gauge-vs-counter note; remove §`GET /queries`.
- **CONFIGURATION.md** §`[query_log]` — only the sentences that describe
  `/queries` as the reader of the ring.
- **CONTEXT.md** — only if "telemetry" needs a definition distinct from
  CONTEXT.md's existing Metrics/Statistics split.
- The `p2-09` row in this phase's `CLAUDE.md`.

## Acceptance criteria

- `/telemetry` returns `401` without an API key, like `/stats`.
- Its `cache` block equals `GET /api/v1/cache` field-for-field.
- Its `memory` block equals `GET /api/v1/debug/memory` **minus exactly
  `allocator_committed_bytes` and `allocator_committed_peak_bytes`** — the drift
  guard for the producer boundary. It must fail loudly if a future allocator
  field lands on the wrong side.
- `memory.residual_bytes == process_rss − accounted_bytes` **within the same
  response** — proves `collect()` gathered RSS and the components together
  rather than across two reads.
- Every `latency` stage carries `count` and `sum_seconds`; no averages.
- `process.uptime_seconds` present and non-negative.
- `GET /api/v1/queries` returns `404`.
- Exactly one `/proc/self/status` parser remains in the workspace.
- E2E: boot the real binary, drive traffic, assert `/telemetry` reflects it —
  `counters.dns.block` rises after a blocked query, `upstreams[]` is non-empty,
  `latency.dns.forward.count` moves.
- Gates green.

**On-device check** (`p2-08` was HTTP-scoped and does not cover this): after
deploy, `GET /api/v1/telemetry` against the running container — confirm every
field a dashboard would chart is populated and agrees with the equivalent
`/metrics` family read at the same instant.

No bench: read path only, nothing on the hot path changes.

## Out of scope

- **The external time-series sink** (Postgres/JSONB, partitioning, a
  `write-on-db` config flag). Discussed and deferred. `/telemetry` is designed
  so a scraper *can* feed one — cumulative counters plus `uptime_seconds` to
  detect resets — but FastAdHunter grows no database client, no credentials and
  no retry buffer.
- **The RouterOS memory-growth question.** Evidence points at page cache from
  list-cache rewrites in
  [`lifecycle/cache.rs:37`](../../../crates/fah-rules/src/lifecycle/cache.rs),
  which rewrites the entire raw text of every list on each refresh — tens of MB,
  roughly 100× all telemetry writes combined (perf history is ~230 KB/day).
  Moving telemetry off disk would not measurably change it. Its own task.
- **Migrating any dashboard client.** The in-repo TUI collapses three pollers
  into one and deletes ~70 lines of `line.starts_with(...)` Prometheus parsing,
  but nothing here depends on it, and the endpoint is designed against the
  generic dashboard need rather than that client's current field list. Separate
  pass. (Two pre-existing bugs found there while reading it, worth fixing then:
  `history_perf_worker` has **no loop** — it fetches once at startup and the UI
  fabricates points from live RSS every 5 min; and `parse_summary_data`
  `.or_else()`-chains three guesses per field because the response shape was not
  discoverable.)
- Reinstating any per-query storage — see the note at the top.
- Raw latency buckets or server-computed percentiles on `/telemetry`.
- A config migration. `[query_log]` must be deleted from the deployed TOML by
  hand before the new binary starts; nothing rewrites it.

## Suggested prompt

> Read `plan/wip/phase2/p2-09-*.md`, API.md §Health & telemetry, and the root
> CLAUDE.md hard rules. Add `GET /api/v1/telemetry` in `fah-api`, gathered in
> one `TelemetrySnapshot::collect()`, reusing `CacheStatsResponse` and
> `MemoryComponentsResponse::of()` rather than defining parallel types — and
> deriving each independently from its port snapshot, never from another
> endpoint's response. Reach the engine counters, latency totals, ruleset and
> upstreams through a `TelemetrySource` method returning an L1 DTO; `fah-api`
> may not import `fah-metrics`. Read latency as `count` + `sum_seconds` only —
> do not allocate the histogram bucket vectors. Fold the two `/proc/self/status`
> parsers into one in `fah-common`, keeping the `Option` return. Remove
> `GET /api/v1/queries` and its plumbing, leaving the stats ring alone. Then
> list the `.md` edits you propose and wait.
