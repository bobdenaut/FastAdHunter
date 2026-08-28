---
title: P5-09 Settings and Diagnostics - Plan
type: feat
date: 2026-08-28
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
origin: plan/wip/phase5/p5-09-settings-and-diagnostics.md
---

# P5-09 — Settings and Diagnostics · Development Plan

---

## Goal Capsule

- **Objective:** the System section — a hand-written Settings form with a raw
  All-settings panel, changed-keys-only writes and gated `[api]` fields, plus
  the three Diagnostics views: Health (status and the counters that explain
  it), Memory (RSS composition, budget rail, persisted trend) and Live Feed
  (the only `query` subscriber, a bounded ring, cards on the phone).
- **Means:** four lazy Preact routes on the p5-05/06/07/08 architecture —
  route-declared lifecycle, shared bounded refresh, lazy uPlot, the p5-07
  dialog/mutation idioms. **No Rust change is expected or planned.**
- **Authority hierarchy:** task file `p5-09-settings-and-diagnostics.md` > the
  API (API.md; where prose and Rust disagree, the Rust) > CONFIGURATION.md +
  the `fah-config` schema (for Settings metadata) > the artboards (structure,
  placement, labels) > information-architecture.md / visual-system.md >
  existing implementation patterns. Artboard figures are drawings, never
  measurements, and **never an API source** (phase constraint 8).
- **Stop conditions:** a figure with no documented field or stated §6
  derivation row → stop, record the conflict, do not invent. A needed Rust
  change → stop and surface. A gate failure surviving three attempts →
  BLOCKED per plan/CLAUDE.md.
- **Tail ownership:** implementation ends at
  `docs/code-review/phase5/p5-09-settings-and-diagnostics-review.md` per phase
  CLAUDE.md §TASK COMPLETION. No commit, no push, no `.md` edit outside that
  review file without the owner's explicit yes.

---

## Product Contract

### Summary

Build `/settings`, `/diagnostics/health`, `/diagnostics/memory` and
`/diagnostics/live-feed` as lazy routes. Settings reads `GET /config`, writes
`POST /config` with only changed keys, holds `config_changed` while mounted,
and carries the Access panel (`POST /auth/password`,
`POST /config/apikey/rotate`, `POST /auth/logout-all`). Health reads the
shared `health`, `telemetry` and `lists` refreshes. Memory reads
`GET /debug/memory` once on entry plus `GET /history/perf` as a range query.
Live Feed owns the `query` subscription and holds a fixed client ring.

### Problem Frame

The most opinionated pages in the phase. The risks the task names: a Settings
form that believes the API described its own constraints (it does not), a raw
panel that prints auth material, a write that overwrites keys the UI does not
model, a memory chart that draws a budget as a wall or a peak as a slice, and
a query feed that keeps the engine publishing for a page nobody is looking at.

### Requirements

Traceability: R1–R16 restate the task file's Scope and Acceptance criteria;
nothing here is new scope.

**Settings**

- R1. Hand-written per config section, grouped as CONFIGURATION.md organises
  them. Every bound, enum and mutability class is hand-carried from
  CONFIGURATION.md and `crates/fah-config/src/schema/` + `lib.rs::validate`
  (§6 metadata table names the source per field); nothing is sourced from the
  API response.
- R2. Writes send only changed keys — asserted by inspecting request bodies in
  a test. Dirty state is tracked per field against the last-received baseline,
  never derived by diffing a re-fetch.
- R3. `[api]` fields are present but gated: saving a dirty `api.*` key opens a
  confirmation naming the lock-out; `api.tls = false` gets the strongest
  wording (session login gone after restart, no HTTP fallback, bearer only).
- R4. A read-only All-settings panel renders the complete `GET /config`
  response; it strips any `auth` key as a second check, and renders the absent
  `policies` key as "none configured".
- R5. `rules.lists`, `policies` and `auth.*` are absent from the form; the
  rules card states the 422-by-design fact and points at the owning pages.
- R6. The restart-required banner is global UI state with no timer: armed by a
  `restart_required: true` POST response or `config_changed` event, cleared
  when a `/health` reading shows the process booted after arming, revalidated
  on entering Settings and opportunistically on any shared-refresh `/health`
  read. It has no Dismiss.
- R7. Access panel: change password (current password required, ≥ 12 chars
  client hint, every session dies on success), rotate API key (shown-once +
  breaks-existing-clients warning **before** confirming), sign out everywhere.

**Health**

- R8. Every displayed figure traces to `/health`, `/telemetry`, `/lists` or
  the one-shot `/config` strategy read (§6). Synthesized vs relayed outcomes,
  the one shed counter, HTTP refusals, SWR drop/fail, the lists problem
  summary, ruleset and resident/peak memory.
- R9. `degraded` is explained, not alarmed, and the explanation is **one
  module in the code** shared with the Upstreams page — never a second copy.

**Memory**

- R10. The composition donut's slices sum exactly to `process_rss`; residual
  is rendered distinctly (hatched) as a remainder; peak is a rail marker,
  never a slice. Budget lines (128 MB steady-state, 256 MB ceiling) are
  markers, never walls, with the MB-vs-MiB note.
- R11. The persisted breakdown chart stacks to `rss_bytes` with residual as
  the top band; `peak_rss` is its own dashed series beside the stack, never
  derived from it; a drop in it is annotated as a restart. Page-fault rate is
  the derivative of `minor_page_faults`, never the cumulative counter.
  Allocator figures are labelled values marked as carrying no compatibility
  promise, never charted.

**Live Feed**

- R12. Only this page subscribes to `query`; the subscription is added on
  mount and dropped on unmount, and leaving the route closes the socket when
  nothing else needs events — proven against a running API.
- R13. Ring of 500 rows on desktop, 200 below 768 px; starts empty; retains
  nothing; filters (verdict, pipeline, client, domain substring) are
  client-side over the rows held; `cached` is a marker, never a verdict.
- R14. Pause freezes rendering only — the ring keeps filling. Hidden page
  stops rendering; what arrived while hidden past the socket grace close is
  simply missed; both facts are stated on the page.

**Cross-cutting**

- R15. Route-scoped per the phase invariant: Settings holds `config_changed`
  and nothing else; Health and Memory hold no event subscription (socket
  closed there); leaving any of the four stops its traffic within one refresh
  interval.
- R16. Correct in both themes at 1400 / 900 / 390 px; the phone Live Feed
  matches `sketch/MobileLiveFeed.dc.html` (cards, 200-row ring, hidden-pause
  statement, 44 px controls). Gates green; bundle recorded gzip **and**
  brotli.

### Scope Boundaries

Out: editing from the raw panel; any config key the endpoint 422s
(`rules.lists`, `policies`, `auth.*`); a log viewer or message store (the
Health artboard states both); server-side feed search or history; new Rust
routes, fields or events; `.md` edits beyond the review file (proposals are
listed in §13 and **not** applied).

---

## Planning Contract

### Sources and precedence

Read for this plan (all verified against the working tree on `phase5-09`):

- Task `plan/wip/phase5/p5-09-settings-and-diagnostics.md`; phase
  `plan/wip/phase5/CLAUDE.md` (invariant, gates, risks).
- `docs/dashboard/open-questions.md` §Closed (config-form generality,
  Diagnostics nesting, bundle shape, `/events` firehose) and §3 (ring size
  "undecided" — superseded by the task's 500/200, which is later and more
  specific).
- `docs/dashboard/information-architecture.md` §Settings, §All settings,
  §Writing config, §Diagnostics, §Cross-cutting behaviour.
- API.md: §Error format, §Health & telemetry, §History, §Configuration,
  §Events, §Debug, §Session authentication.
- CONFIGURATION.md §Precedence, §Mutability classes, §Reference.
- PERFORMANCE.md §Budgets, §Reading a memory figure.
- Rust: `fah-api` `routes.rs` (`get_config`, `post_config`, `rotate_api_key`,
  `health`, `debug_memory`, auth handlers), `config_store.rs` (`BOOT_KEYS`,
  `apply_patch`, `merge`, `changed_paths`), `events.rs` (subscribe protocol,
  `has_query_subscribers`), `wire.rs` (`MemoryResponse`,
  `DebugMemoryResponse`, query-event shape, perf fields), `ports.rs`
  (`verdict_str`), `fah-config` `schema/*` + `lib.rs::validate`,
  `fastadhunter/main.rs:671` (engine gate).
- Frontend: `router/routes.ts`, `lifecycle/*`, `events/*`, `refresh/*`,
  `api/*`, `shell/shell.tsx`, `components/*`, `charts/*`,
  `pages/performance/*`, `pages/upstreams/degraded-banner.tsx`,
  `scripts/postbuild.mjs`, `constants.ts`.
- Artboards: `Settings`, `Health`, `Memory`, `LiveFeed`, `MobileLiveFeed`.
- Implementation Summaries of p5-05 through p5-08 reviews.

### §3 Conflicts, resolved

Every conflict found between task, API contract, config schema, runtime
semantics and artboards. Resolution rule: behaviour and data follow the task
and the code; the artboard keeps layout, placement and wording only.

| # | Conflict | Evidence | Resolution |
| - | -------- | -------- | ---------- |
| C1 | Settings artboard draws `dns.cache.serve_stale_seconds` with the note "a stale entry answers only after a failed forward" | No such key. `DnsCacheConfig` has `serve_stale: bool`, `min_ttl_seconds`, `max_ttl_seconds`, `negative_ttl_max_seconds`, `swr_workers`, `cleanup_interval_seconds`; SWR answers stale hits immediately (ADR-0005) | Form fields come from the schema; the artboard's field list is illustrative. Help text follows the schema doc comments |
| C2 | LiveFeed/MobileLiveFeed artboards draw a `REFUSED` verdict row ("egress policy · IP literal host") | `ports.rs::verdict_str` is `pass`/`allow`/`block` only; API.md: `counters.http.refused` "is counted on the proxy, not on the event stream" | No REFUSED row, pill or filter. The feed's verdict vocabulary is exactly `pass`/`allow`/`block`. Refusals surface on Health |
| C3 | MobileLiveFeed card carries "policy kids" metadata | The query event has no `policy` field (wire.rs `QueryEventJson`; API.md event shape) | Dropped. No policy metadata on feed rows |
| C4 | Both LiveFeed artboards print "0 shed by the engine" in the feed header | `events_dropped` lives on `/telemetry`; the Live Feed declares no polled endpoint and the task fixes `query` as its only subscription — the figure has no route-scoped source | Dropped from the feed. The ring meter keeps "N / cap rows held"; the footnote points at Health for the shed counter |
| C5 | Health artboard: "0 healthy, 1 penalized, 1 probing, 1 recovering" | `state` is `healthy` \| `penalized` \| `probing` (API.md, `types.ts`); "recovering" does not exist, and the counts do not even sum to the drawn "3 endpoints" | Summary line shows the three real states, and only under `strategy = "adaptive"` (p5-08's `upstreamMode` gate); under `fallback` it shows "N endpoints" with no state counts |
| C6 | Settings artboard's restart banner has a Dismiss button | Task + IA: "a persistent banner holds until a restart is observed via `/health` uptime resetting" | No Dismiss. The banner clears only on an observed restart |
| C7 | Settings artboard shows a masked API-key value `fah_••••…` | No endpoint returns the current key; `GET /config` does not carry it (`ApiConfig` = address/port/tls) | No masked value — rendering one implies the UI holds the key. Label, description and Rotate button only |
| C8 | Memory artboard labels 97 MiB "highest the sampled series **ever** recorded" | The chart fetches one `[from, now)` window; nothing serves an all-time series maximum | Labelled as the fetched window's maximum ("highest sampled in this window"), re-computed per range selection |
| C9 | Settings artboard's section nav omits `engine` and `dns.listen` | CONFIGURATION.md §Reference includes both; the decision is "grouped as CONFIGURATION.md organises them" | Both sections are in the form (both boot). The artboard nav is a partial drawing — it also draws only 4 of its own 11 section cards |
| C10 | Route table (`routes.ts`) declares Memory `endpoints: []` and Health `['health','telemetry']`, but Memory needs `/debug/memory` + `/history/perf`, and Health's lists-problem summary needs `/lists` | Task scope names the list-problem summary explicitly; `/history/*` is a range query, not a poll (p5-08 precedent); `/debug/memory` is not a `REFRESH_ENDPOINTS` member | Health adds `lists` to its declaration (existing shared read, explicitly required). Memory stays `endpoints: []`: `/debug/memory` is a one-shot on entry, `/history/perf` a range query — the Performance page's exact pattern |
| C11 | `api/history.ts` pins `PERF_FIELDS` to five fields and its comment says the memory fields "belong to the Diagnostics page" | `resources.test.ts` pins the list; the comment anticipates this task | The Memory page requests its own field set (`rss_bytes`, `peak_rss`, `memory`, `minor_page_faults`); the pinned test is edited deliberately (§7.3) |
| C12 | Memory artboard's top-bar shows "up 4h 31m" | The shell's TopBar shows version only; uptime would need a `/health` reading the Memory route does not declare | Not added. Uptime lives on Health, where `/health` is declared |
| C13 | Task text says Diagnostics answer-outcome/shed/refusal figures are "since boot"; Health artboard adds "these are also persisted per interval" | Both true: `/telemetry` counters are process-lifetime; `answers_delta` persists in `/history/perf` | Health renders the lifetime counters (its declared reads) and keeps the artboard's one-line pointer to persistence as prose — no history fetch on Health |
| C14 | open-questions §3 lists the ring size as undecided | Task fixes 500 desktop / 200 narrow | Task wins — later and specific. §13 proposes closing the open-questions bullet (not applied here) |
| C15 | CONTEXT.md §Query Log describes a "bounded, persisted record" | FAH persists no per-query record; the task mandates "Live Feed, not Query Log" wording | UI uses "Live Feed" everywhere and never promises history. CONTEXT.md reconciliation is a §13 proposal, not applied |

### §4b Deviation registry — artboard departures beyond §3

| # | Artboard | Shipped | Why |
| - | -------- | ------- | --- |
| X1 | Health's Rule-lists card carries no refresh cluster | A `lists` cluster on that card | One cluster per distinct polled endpoint per page (p5-06 rule); the page polls three endpoints, so it carries three clusters — health on the status card, telemetry on "What clients received", lists on "Rule lists" |
| X2 | Settings banner names the exact keys ("dns.cache.max_entries and dns.upstreams.strategy") | Keys named only when this browser submitted them; a `config_changed` event carries only `restart_required`, so an externally-armed banner says "a saved change needs a restart" without naming keys | The event is a nudge with no payload beyond the flag (API.md §Events) |
| X3 | Memory footprint rail is one hand-drawn SVG | Built from the same primitives as the p5-08 budget hooks: a horizontal scale with the composition segments, the window-max and peak markers, and the two dashed budget rules — an SVG component, not a uPlot chart | A rail is not a time series; uPlot buys nothing here |
| X4 | LiveFeed artboard's desktop `Detail` column mixes qtype, `cached`, `endpoint N`, method/type/status/bytes | Same composition, from documented fields only (§6 D20) | The artboard's content matches the event shape except C2/C3 |
| X5 | Feed rows draw domain+path in one cell with the path dimmed | Same | Matches the event's `domain` + `path` split |
| X6 | Mobile ring size switches at "narrow viewport" | Sampled once per page mount via `matchMedia('(max-width: 767px)')`; no resize listener | The application has no viewport listener anywhere (p5-08 F5 decision); a rotation mid-visit keeps the mount-time capacity until the next entry, and the on-page capacity line prints whichever is in force |
| X7 | Artboard Settings shows per-section cards all expanded with a side nav | Side nav anchors scroll to sections (desktop ≥ 1200 px); below that the nav collapses and sections stack | visual-system §Responsive |

### Key Technical Decisions

- **KTD1 — metadata lives in one hand-written module,
  `pages/settings/metadata.ts`.** Per-field: dotted key, section, control type
  (int / bytes / bool / enum / ip / string / tz / string-list / server-list),
  bounds, enum values, mutability class, help line, and the doc/source anchor
  it was carried from. The API response is values only; this module is the
  schema. It is the single drift point the task accepts, and the review file
  will cite it as "where the bounds came from".
- **KTD2 — dirty state is per field against a baseline, and the baseline is
  the last `GET /config`.** A field is dirty iff its edited value differs from
  the baseline value. `config_changed` triggers a re-read; the new response
  replaces the baseline and the displayed value of every **clean** field;
  dirty fields keep the operator's edit (and a field whose edit now equals the
  server value stops being dirty). The submit body is built from dirty fields
  only, nested (`{"dns":{"cache":{"max_entries":20000}}}`); arrays
  (`dns.upstreams.servers`, `egress.allow_destinations`) are sent whole when
  any element is dirty, because the server merge replaces arrays wholesale
  (`config_store.rs::merge`).
- **KTD3 — the restart banner is a module store plus one registry tap.**
  `system/restart-banner.ts` holds `{armedAtMs, keys[]} | null` in module
  state (in-memory; a hard reload forgets it — accepted limitation, recorded).
  Cleared when a `/health` reading satisfies
  `nowMs − uptime_seconds·1000 > armedAtMs` — the boot happened after arming.
  Both times are client-clock and `uptime_seconds` is a server-measured
  duration, so the comparison is skew-free. Feeding it: (a) Settings does a
  one-shot `getHealth` on entry while armed; (b) `RefreshRegistry` gains
  `observe(endpoint, listener)` — a listener set that receives announcements
  and **never** refcounts, starts a timer or triggers a fetch — and the
  banner store observes `health`. The shell renders the banner above the
  content column on every route.
- **KTD4 — the degraded explanation moves to one shared module.**
  `pages/upstreams/degraded-banner.tsx` (component + `explanation(mode)`)
  relocates to `components/degraded-banner.tsx`; Upstreams' import is
  updated and Health reuses it verbatim. That is the acceptance criterion
  "one string in the code, not three" made structural.
- **KTD5 — Memory is the Performance pattern, not a poll.** Entry one-shots:
  `GET /debug/memory` (instant: donut, rail, kernel/allocator table) and
  `GET /config` is **not** needed (nothing on the page depends on config).
  `GET /history/perf?fields=rss_bytes,peak_rss,memory,minor_page_faults`
  re-fetches on range change (24 h / 7 d / 30 d chips, `ranges.ts` reuse) and
  at no other time. No timer, no endpoint declaration, `activeTimers() === 0`.
- **KTD6 — the Live Feed ring is a bounded buffer flushed on animation
  frame.** `socket.on('query', …)` appends into a fixed-capacity ring
  (capacity 500/200, X6); a pending `requestAnimationFrame` coalesces renders,
  so a burst costs one render per frame, and a hidden document — where rAF
  does not fire — stops rendering by construction while the ring (still
  bounded) keeps absorbing until the existing 30 s grace close tears the
  socket down. Pause freezes the rendered snapshot only; Clear empties ring
  and buffer. No `setTimeout`/`setInterval` anywhere on the page (the
  scheduler token stays confined to `lifecycle/timers.ts`).
- **KTD7 — restart boundaries in the Memory series come from `peak_rss`.**
  `peak_rss` is monotone within one process lifetime (API.md §history/perf);
  a decrease between consecutive samples is a restart, never a reclaim. The
  boundary indexes drive the chart annotation and scope the leak-watch deltas
  ("this process" = since the last boundary). A `0` `peak_rss` row (pre-field
  or no getrusage) is excluded from boundary detection rather than read as a
  drop.
- **KTD8 — hidden-page and route-leave behaviour is inherited, not built.**
  The route lifecycle already closes the socket when the union empties,
  suspends on hidden with the 30 s grace, and aborts shared-refresh reads on
  last unsubscribe. This task adds no lifecycle machinery beyond KTD3's
  passive `observe`.

### High-Level Technical Design

Four new page chunks replace the `pages/system.tsx` placeholder loads:
`pages/settings.tsx` (+ `pages/settings/*`), `pages/diagnostics-health.tsx`
(+ `pages/health/*`), `pages/diagnostics-memory.tsx` (+ `pages/memory/*`),
`pages/live-feed.tsx` (+ `pages/live-feed/*`). All four are `built: true`,
`ownsHeader: true` (each artboard writes its own header/subtitle). The
placeholder `system.tsx` chunk is deleted with its last consumer.

Data flow summary:

| Route | Events | Polled (shared refresh) | Entry one-shots | Range queries | Writes |
| ----- | ------ | ----------------------- | --------------- | ------------- | ------ |
| `/settings` | `config_changed` | — | `GET /config`; `GET /health` only while banner armed | — | `POST /config`, `POST /auth/password`, `POST /config/apikey/rotate`, `POST /auth/logout-all` |
| `/diagnostics/health` | — | `health`, `telemetry`, `lists` | `GET /config` (strategy, once — boot-only, cannot change under a running process) | — | — |
| `/diagnostics/memory` | — | — | `GET /debug/memory` | `GET /history/perf` (fields per KTD5) | — |
| `/diagnostics/live-feed` | `query` | — | — | — | — |

---

## §6 Every displayed value — provenance and derivation

Derivation rows are `D1…`; each becomes one pure function in `derive.ts`
(alongside `R*`/`E*`) or is marked *(format)* → `charts/format.ts` /
`time.ts`, *(count/layout)* → counted where printed. Anything not in this
table and not a verbatim field is a conflict, not an implementation choice.

### Settings

| Display | Source |
| ------- | ------ |
| Every field value | `GET /config`, verbatim per dotted key |
| Bounds / enums / mutability tags (LIVE / RESTART) | `metadata.ts` (KTD1) — hand-carried from CONFIGURATION.md §Reference + §Mutability classes, `fah-config/src/schema/*`, `lib.rs::validate`, `config_store.rs::BOOT_KEYS` |
| "N lists — not editable here" | D1 — `config.rules.lists.length` from the same `GET /config` response *(count)* |
| Banner text | KTD3 store: keys from this browser's own POST bodies; generic wording when event-armed (X2) |
| Save result ("applied live" / "needs restart") | `POST /config` response `applied` / `restart_required`, verbatim |
| 422 anchoring | D2 — parse the envelope message for `` `dotted.key` `` (server format: ``invalid value for `key`: …``, `error.rs`); a match anchors the message under that field, no match renders as a form-level error |
| Raw panel rows | `GET /config` response, verbatim; `auth` stripped (R4); absent `policies` rendered "none configured" |
| Rotate result | `POST /config/apikey/rotate` → `{api_key}`, rendered once, never stored |

### Health

| Display | Source |
| ------- | ------ |
| Status pill + circle, version, uptime | `GET /health` `status`/`version`/`uptime_seconds`; uptime via `formatUptime` *(format)* |
| Degraded explanation | shared `components/degraded-banner.tsx` (KTD4), keyed by `upstreamMode(config.dns.upstreams.strategy)` |
| Endpoint summary line | D3 — counts of `telemetry.upstreams[].state` by value, **adaptive only**; under `fallback`/`unknown`: "N endpoints" with no state counts (C10 gate, p5-08 KTD5) |
| SERVFAIL synthesized / relayed, REFUSED relayed | `telemetry.counters.dns.answers.*`, verbatim |
| events dropped | `telemetry.counters.events_dropped` |
| HTTP requests refused | `telemetry.counters.http.refused` |
| SWR dropped / failed | `telemetry.counters.swr.dropped` / `.failed` |
| Rule lists "M of N need attention" | D4 — from `GET /lists` items: N = total, M = count with `status` ∈ {`failed`, `rejected`} *(count)*; problem rows show `id`, status pill, `last_error` |
| compiled rules / duplicates / last compile | `telemetry.ruleset.rules` / `.duplicates_removed` / `.compile_duration_seconds` (seconds label *(format)*) |
| resident / peak memory | `telemetry.memory.process_rss` / `.process_peak_rss`, `formatMiB` *(format)*; `null` renders "unavailable off Linux", never 0 |
| "updated Ns ago" + interval + refresh | `RefreshCluster` per endpoint (X1) |

### Memory

| Display | Source |
| ------- | ------ |
| Donut slices: ruleset / DNS cache / stats / residual | `GET /debug/memory`: `ruleset_bytes`, `cache_estimated_bytes`, D5 = `stats_aggregates_bytes + stats_clients_bytes`, `residual_bytes`. D6 — slice shares = value ÷ `process_rss`. Sum property: `accounted_bytes + residual_bytes = process_rss` holds server-side (`MemoryComponentsResponse::of`); the donut renders those four and **must not** re-derive residual |
| Donut centre + "accounted" row | `process_rss` (`formatMiB`), `accounted_bytes` verbatim |
| slice captions ("752,585 rules", "1,108 entries") | `telemetry`-free: rules count is **not** on `/debug/memory` → use `cache_entries` (present) for the cache caption; the ruleset caption carries no rule count on this page (Health has it). *Resolved conflict: the artboard's "752,585 rules" caption has no source on this page's reads — dropped* |
| Composition unavailable state | `process_rss === null` (non-procfs dev box) → the donut and rail render an explicit "RSS unavailable on this platform" state, never zeros |
| Rail: current RSS segment stack | same four component values scaled to the axis (X3) |
| Rail: window-max marker | D7 — `max(items[].rss_bytes)` over the fetched history window, labelled with the active range (C8) |
| Rail: peak marker | `process_peak_rss` from `/debug/memory` |
| Rail: budget rules 128 MB / 256 MB | D8 — constants in a `budgets` module, cited to PERFORMANCE.md §Budgets; decimal MB, with the MiB-reading note verbatim from the task ("~4.9 % gap") |
| "45 % of the steady-state budget" | D9 — `process_rss / 128 MB` *(one multiplication, in derive)* |
| Leak watch: residual now | `residual_bytes` (`formatMiB`) |
| Leak watch: residual / components delta "this process" | D10 — last-sample minus first-sample-after-last-restart-boundary (KTD7) of `memory.residual_bytes` and `memory.accounted_bytes` from history items |
| Leak watch: series min / max | D11 — min/max of `items[].rss_bytes` over the window |
| Leak watch: minor fault rate | D12 — `(mpf[i] − mpf[i−1]) / (ts[i] − ts[i−1])` for the latest adjacent pair; a negative delta (restart) → no rate, rendered "—" |
| Trend chart bands | D13 — stack order bottom-up: ruleset, cache, stats (D5 per row), residual on top; cumulative sums build the fill series; the top edge equals `rss_bytes` by the server-side identity — asserted in tests, not re-derived |
| Trend chart modes | stacked (default) / share / separate chips: share = D14, each band ÷ that row's `rss_bytes`; separate = one small chart per component, same data |
| Peak series | `items[].peak_rss` as a dashed line series (never an area, never in the stack); rows with `0` → gap (`0` = pre-field or unavailable, API.md) |
| Restart annotation | KTD7 boundary → vertical rule + "restart" caption |
| Fault-rate chart | D12 applied pairwise across the window; restart pairs → `null` gap |
| Kernel/allocator table | `/debug/memory`: `major_page_faults`, `minor_page_faults`, `process_peak_rss`, `allocator_committed_bytes`, `allocator_committed_peak_bytes`; `null` → "unavailable", never 0; the two allocator rows carry a NO CONTRACT tag and the equality note (API.md §Allocator fields) |

### Live Feed

| Display | Source |
| ------- | ------ |
| Row fields | the `query` event verbatim: `ts` (via `clockLabel` *(format)*), `kind`, `client_name ?? client` (D15 — prefer name), `domain` + `path`, `verdict` pill, `rule`, `list`, `duration_ms` |
| Detail cell | D20 — DNS: `qtype`, `cached` marker, `endpoint N` when the key is present; HTTP: `method · resource_type · status · bytes` (bytes via a `formatBytes` helper *(format)*) |
| Verdict pills | `pass` neutral / `allow` green / `block` red (visual-system §Live Feed); vocabulary is closed (C2) |
| "N / cap rows held" | D16 — ring length / capacity *(count)* |
| Filters | D17 — client-side predicates over held rows: verdict equality, kind equality, client substring (against name and IP), domain substring |
| Pause / Clear | render-state only / ring reset (KTD6, R14) |
| Capacity + hidden-pause statements | static text stating the in-force capacity (X6) and the hidden-page behaviour (R14) |
| Empty state | "starts empty — rows appear as the household resolves" (no retained history, R13) |

---

## §7 Route, lifecycle and API-client architecture

### 7.1 Route table changes (`src/router/routes.ts`)

- `/settings`: `built: true`, `ownsHeader: true`, `events: ['config_changed']`
  (already declared), `endpoints: []`, `load: () => import('../pages/settings')`.
- `/diagnostics/health`: `built: true`, `ownsHeader: true`, `events: []`,
  `endpoints: ['health', 'telemetry', 'lists']` (C10),
  `load: () => import('../pages/diagnostics-health')`.
- `/diagnostics/memory`: `built: true`, `ownsHeader: true`, `events: []`,
  `endpoints: []`, `load: () => import('../pages/diagnostics-memory')`.
- `/diagnostics/live-feed`: `built: true`, `ownsHeader: true`,
  `events: ['query']` (already declared), `endpoints: []`,
  `load: () => import('../pages/live-feed')`.
- `pages/system.tsx` is deleted; `routes.test.ts` pins the new rows.

Because `effectiveEvents`/`effectiveEndpoints` gate on `built`, flipping each
row is the moment that page's subscriptions go live — each row flips in its
own implementation unit, never before the page exists.

### 7.2 Lifecycle additions

- `RefreshRegistry.observe(endpoint, listener): () => void` — passive tap
  (KTD3), and a **watch item**: it must never grow into a second
  polling/lifecycle mechanism. The spec that keeps it one:
  - **No initial replay.** Unlike `subscribe`, `observe` never calls back
    synchronously with retained state — it sees only future `announce()`s,
    so it cannot serve as a data source. The banner's current-value need is
    Settings' entry one-shot.
  - Observers live in a separate `Map<RefreshEndpoint, Set<Listener>>`
    referenced in exactly two places: `observe()` and `announce()`. No read
    in `subscribe`, `startTimer`, `fetch`, `isStale` or `setSuspended`.
  - The return value is the unsubscribe and nothing else; no path from an
    observer to `invalidate`.
  - `services.ts` hands the banner store `Pick<RefreshRegistry, 'observe'>`,
    so the store cannot reach `subscribe`/`invalidate` by type.
  - Caller pin: a source-invariant test (scheduler-token-grep style) asserts
    `.observe(` is called only from `services.ts` and tests.
  Tests: observer with zero subscribers → no fetch ever (counting stub) and
  `activeTimers() === 0`; last subscriber leaving with an observer attached
  still stops the timer and aborts the in-flight controller;
  `subscriberCount()` unchanged by observe; `setSuspended(false)` with only
  observers restarts nothing; no synchronous callback on registration.
  V5's `fahTimers() === 0` on Memory with the banner armed is the live proof.
  Fallback if implementation resists these bounds: tap successful reads
  inside `api/health.ts` instead — simpler, but a hidden side effect in an
  API module (principle 10), so it is the retreat, not the default.
- `system/restart-banner.ts` — store + `arm(keys)`, `observeHealth(state)`,
  `clearIfRestarted(health, nowMs)`, `subscribe(listener)`. Wired once in
  `services.ts` (`refresh.observe('health', …)`); rendered by the shell above
  the routed content, `role="status"`.

### 7.3 API client additions (`src/api/`)

- `types.ts`: full `Config` shape (all sections/fields per the schema —
  replaces the deliberately-narrow p5-06 type; the doc comment moves from
  "p5-09 owns the full shape" to naming `metadata.ts` as the constraints
  side). `DebugMemory = Memory & { allocator_committed_bytes: number | null;
  allocator_committed_peak_bytes: number | null }`. `PerfItem` gains
  `rss_bytes?`, `peak_rss?`, `memory?: PerfMemory`, `minor_page_faults?`
  (`PerfMemory` = the six persisted component keys; `residual_bytes` derived
  server-side on read, still `number` there — rows predating the field read
  back as zeros, API.md). `ConfigUpdateResponse = { applied, restart_required }`.
  `ApiKeyResponse = { api_key }`.
- `config.ts`: `postConfig(patch)` → `POST /api/v1/config`.
- `debug.ts` (new): `getDebugMemory(signal)` → `GET /api/v1/debug/memory`.
- `auth.ts`: `changePassword(current, next)` → `POST /api/v1/auth/password`
  (`notifyUnauthorized: false` — its 401 means "wrong current password" and
  must not bounce to login); `logoutAll()` → `POST /api/v1/auth/logout-all`;
  `rotateApiKey()` → `POST /api/v1/config/apikey/rotate` (lives in
  `config.ts` beside its route family).
- `history.ts`: a second pinned field list
  `MEMORY_PERF_FIELDS = ['rss_bytes','peak_rss','memory','minor_page_faults']`
  and a widened `PerfField` union; `resources.test.ts` pin updated (C11).
- `request_coverage.rs` needs nothing: no new Rust route.

---

## §8 Page by page

### 8.1 Settings (`/settings`)

Layout (artboard): restart banner (shell-owned, KTD3) → 200 px anchor nav +
section cards → Access card → Save/Discard bar → All-settings panel at the
bottom (IA: "at the bottom" — below Access).

**Sections and fields** (metadata source in parentheses; class from
`BOOT_KEYS`):

- `[engine]` `mode` — enum `dns | dns+http | dns+http+https` (schema
  `EngineMode`); boot.
- `[dns.listen]` `address` (IP literal, `validate_ip`; `::` note), `port`
  (1–65535, `validate_nonzero_port`); boot.
- `[dns.blocking]` `mode` — enum **`null_ip` only** (schema `BlockingMode`;
  CONFIGURATION.md's future values are documentation, not accepted values —
  the select offers exactly what the schema deserialises), `ttl_seconds`
  (u32); boot.
- `[dns.cache]` `max_entries` (u32), `max_bytes` (u64, ≥ 1 MiB —
  `MIN_CACHE_MAX_BYTES`), `min_ttl_seconds` (≤ `max_ttl_seconds`, cross-field
  rule client-checked), `max_ttl_seconds`, `negative_ttl_max_seconds`,
  `serve_stale` (bool), `swr_workers` (u32, 0 disables), 
  `cleanup_interval_seconds` (u32, 0 disables); all boot.
- `[dns.upstreams]` `strategy` — enum `fallback | adaptive`; `timeout_ms`
  (1–10000); `penalty_failures` (1–255); `servers` — bounded row editor,
  1–8 rows (`MAX_UPSTREAM_SERVERS`), each `{address, protocol: udp|dot|doh,
  hostname}`; client rules mirror `validate`: dot requires hostname, doh
  requires an `https://` address; whole array sent when any row is dirty
  (KTD2); all boot.
- `[http.listen]` `address`, `port`; `[http]` `max_connections` (≥ 1),
  `idle_timeout_ms`, `header_timeout_ms`; all boot (whole section,
  `BOOT_KEYS` note on `max_connections` carried into help text).
- `[egress]` `allow_destinations` — line editor (existing `line-editor`
  component), entries IP or CIDR (`validate_allowed_destination` mirrored);
  `allow_ip_literal_hosts` bool; boot. Help text keeps the default-deny
  security framing.
- `[rules]` `refresh_hours_default` (u32) — **runtime**; plus the read-only
  `lists` row (D1) and the one-writer footnote (artboard wording).
- `[schedule]` `timezone` — POSIX TZ string (server-validated `PosixTz`;
  client check is presence/non-empty only — the grammar is not mirrored, a
  bad string anchors the server 422); **runtime**; help text carries the
  Bucharest example and the WEST-positive note.
- `[stats]` `snapshot_interval_seconds` (u32); boot.
- `[history]` `enabled` (bool, **runtime**), `sample_interval_seconds`
  (1–86400, boot), `retention_days` (1–3650, **runtime**).
- `[api]` `address` (IP), `port`, `tls` (bool) — all boot, all **gated**
  (R3): the fields render with a standing warning line, and a Save whose
  dirty set touches `api.*` first opens a `ConfirmDialog` that names each
  consequence: `tls → false` = "session login stops working after the next
  restart — the dashboard cannot authenticate at all; only bearer-key access
  remains"; `address`/`port` = "the API and this dashboard move; this page's
  URL stops answering after the restart". Cancel sends nothing.
- `[log]` `level` — enum `error|warn|info|debug|trace`; `format` — enum
  `text|json`; boot.

**Write path.** Save builds the nested dirty-keys body (KTD2), client-side
validates against `metadata.ts` first (out-of-range → inline error, no
request), POSTs, then: `applied: true` → inline "applied live" note;
`restart_required: true` → `restartBanner.arm(dirtyBootKeys)`. The
`config_changed` event the server publishes triggers the baseline re-read
(KTD2). Errors: 422 anchored per D2; network/5xx → form-level `ErrorState`
idiom. No navigation block: config writes are milliseconds and recompile
nothing (policies recompile is server-side and cheap — `post_config`).

**All-settings panel.** Renders the raw `GET /config` JSON as a per-section
key/value table (dotted keys, monospace values; arrays as their JSON). Second
auth check: any top-level `auth` key is dropped before rendering and the
panel notes nothing (defence, not a feature). Absent `policies` renders an
explicit row: "policies — none configured". Read-only, stated.

**Access panel.** Three rows per the artboard, each a dialog on the p5-07
`ConfirmDialog`/focus-trap idiom:

- Change password — current + new (+ repeat) fields; client hint ≥ 12 chars;
  on `204` the session is dead server-side: show the one-line "every session
  was signed out" state and navigate to `/login`. `401` → "current password
  is wrong" inline; `422` → server message inline; `429`/`503` → message with
  `Retry-After` when present (the `ApiError.retryable` distinction).
- Rotate API key — the warning **before** the confirm ("shown once; the old
  key stops working immediately; anything still using it breaks"), then the
  key rendered once in the dialog, monospace, with a copy button; never
  stored, gone when the dialog closes.
- Sign out everywhere — confirm → `POST /auth/logout-all` → `/login`.

### 8.2 Health (`/diagnostics/health`)

Cards per the artboard: status card (health cluster; uptime, version,
degraded explanation via KTD4 when `status === 'degraded'`; endpoint summary
D3 + "Open Upstreams →" link) · "What clients received" (telemetry cluster;
answers table + synthesized-vs-relayed prose from the artboard) ·
"Backpressure and refusals" (events_dropped, http.refused, swr dropped/
failed + the two artboard notes) · "Rule lists" (lists cluster; D4 summary,
problem rows with `last_error`, "Open Lists →") · "Engine" (ruleset +
resident/peak, "Open Memory →") · the static "What this page will never do"
card (three columns verbatim from the artboard). Strategy is read from
`GET /config` once on mount (boot-only; the Upstreams-page comment applies
verbatim); `unknown` (failed read) degrades D3 to the no-counts form and the
degraded banner to its both-readings text.

### 8.3 Memory (`/diagnostics/memory`)

Cards per the artboard: footprint rail (X3, D7–D9) · composition donut
(`Donut` extended with an optional hatch pattern for the residual segment —
an SVG `<pattern>` def, theme-token colours) + legend table with shares (D6)
+ the two artboard footnotes (residual is a real slice; peak deliberately not
a slice) · leak watch (D10–D12 + the trend sparkline of
`memory.residual_bytes`, restart rule from KTD7) · "Where RSS goes, over
time" (D13/D14 stacked-area via `charts/lines.ts` area specs over
pre-stacked series; peak dashed series; mode chips stacked/share/separate;
range chips 24 h / 7 d / 30 d via `ranges.ts`) · fault-rate chart (D12) ·
kernel/allocator table. Empty/edge states: `history.enabled` cannot be read
here (no config read) so an empty history answer renders the per-card "no
samples in this window" state with the artboard's honesty rule (an empty
range is data absence, not failure); `process_rss === null` renders the
composition-unavailable state.

### 8.4 Live Feed (`/diagnostics/live-feed`)

Desktop: the tail-not-a-log notice (artboard wording, capacity interpolated),
filter card (D17 controls; pause/clear right-aligned), feed card with the
nine-column table (Time · Pipe · Client · Domain/path · Verdict · Rule ·
List · Detail · ms) and the footnote ("`cached` is a marker, not a verdict ·
`endpoint N` appears only on a forwarded DNS answer · a slow tab is
disconnected rather than back-pressuring the engine; the socket reconnects on
its own"). Ring meter D16 in the card header.

Phone (< 768 px, `MobileLiveFeed.dc.html` is the source of truth): one card
per event with a verdict-coloured left border; first line verdict pill +
kind + time; domain wraps (`word-break`); metadata line per D20; chip row in
its own horizontal scroller (verdict + kind chips); Pause / Clear / Filter…
as full-width 44 px controls (Filter… toggles the client/domain text
inputs); ring meter card; the hidden-pause statement card. The body never
scrolls sideways.

Mechanics: KTD6 ring + rAF flush; capacity per X6; pause = stop applying
flushes to the rendered snapshot (buffer and ring keep moving); clear resets
both; unmount releases the `socket.on('query')` listener and the route
transition drops the `query` type — which empties the union and closes the
socket when the next route needs no events.

---

## §9 Mutation semantics — the four writes

| Write | Confirm | Block nav | On success | On failure |
| ----- | ------- | --------- | ---------- | ---------- |
| `POST /config` | only when dirty set touches `api.*` (R3) | no | baseline refresh via `config_changed`; banner arm on `restart_required` | 422 → D2 anchoring; else form-level error |
| `POST /auth/password` | dialog itself | no | state note + `/login` (session dead by contract) | 401/422/429/503 inline per §8.1 |
| `POST /config/apikey/rotate` | yes — shown-once + breakage warning | no | key rendered once | envelope message in dialog |
| `POST /auth/logout-all` | yes | no | `/login` | envelope message in dialog |

None carries an `AbortSignal` — a write must land (p5-07 rule); responses
arriving after unmount mutate nothing route-scoped (no invalidate exists for
these).

---

## §10 Phone and themes

- Settings fields stack label-over-control below 768 px (task); the anchor
  nav collapses (X7); dialogs keep the focus trap and 44 px targets.
- Health/Memory cards go single-column; the rail and donut stay legible at
  390 px without pinch-zoom (task) — the rail scales with the viewport, the
  donut keeps its fixed ~180 px size centred.
- Live Feed phone layout per §8.4.
- Both themes via tokens only; verdict colours carry their word (visual-system
  §Theme); the hatch pattern uses `currentColor`-safe token strokes so it
  survives both themes.
- Measured at 1400 / 900 / 390 px, both themes, in the verification pass.

---

## Implementation Units

Each unit ends with `npm run typecheck`, `npm run test`, `npm run build`
green (and `cargo` gates untouched — no Rust change) before the next.

### U1. API client and types

`types.ts` full `Config`, `DebugMemory`, `PerfItem`/`PerfMemory`,
response shapes; `config.ts` `postConfig` + `rotateApiKey`; `auth.ts`
`changePassword` + `logoutAll`; `debug.ts` `getDebugMemory`; `history.ts`
`MEMORY_PERF_FIELDS`; `index.ts` exports; `resources.test.ts` pins updated
(C11) + new request/body assertions.

### U2. Shared infrastructure

`RefreshRegistry.observe` (+ tests: no timer, no fetch, same state);
`system/restart-banner.ts` (+ tests: arm from keys, arm generic, boot-time
clear D-logic incl. tolerance-free skew argument, no clear while
`uptime` predates arming); shell renders the banner; `services.ts` wires the
observer; `components/degraded-banner.tsx` move (KTD4) with the Upstreams
import updated and its tests still green; `Donut` hatch-pattern option.

### U3. Settings metadata + patch builder (pure)

`pages/settings/metadata.ts` (KTD1, every §8.1 row with source anchors);
`pages/settings/patch.ts` — dirty tracking, nested body builder, array
wholesale rule, client validation against metadata, D2 anchor parser.
Tests: changed-keys-only bodies (R2 acceptance, asserted on the built
object **and** on the fetch body in U4), array wholesale, revert-clears-dirty,
baseline merge preserving dirty fields, every metadata bound rejects/accepts
at its edges, D2 extracts `` `key` `` and falls back cleanly.

### U4. Settings page

`pages/settings.tsx` + section cards + Access panel + All-settings panel +
route row flip. Tests: fetch-stub request-body assertion for changed-keys-only
(acceptance), `config_changed` → one re-read with dirty preservation, raw
panel shows unmodelled keys / strips injected `auth` / "none configured"
policies row, `api.*` gate (cancel sends nothing; confirm names the
lock-out; tls wording), banner arms on `restart_required` response and event
and does not arm otherwise, rotate dialog warns before and shows once,
password flow branches (401/422/success-to-login), `activeTimers() === 0`.

### U5. Health page

`pages/diagnostics-health.tsx` + cards + route row flip (adds `lists`).
Tests: every §6 Health row from stubbed payloads; D3 strategy gate
(adaptive counts / fallback no-counts / unknown); D4 counts; degraded banner
is the shared module (import identity + no second string:
`filtering-invariants`-style grep test that the explanation text exists once
in `src/`); null memory fields render "unavailable"; three clusters, one per
endpoint.

### U6. Memory derivations + page

`derive.ts` D5–D14 + KTD7 boundary detector (+ tests: slices sum to RSS with
server residual, never re-derived; boundary on peak drop, `0` rows excluded;
D10 scoping; D12 restart pair → null; D13 stack top equals `rss_bytes`; D14
shares; window max). `pages/diagnostics-memory.tsx` + cards + route flip.
Tests: donut segments exclude peak (acceptance), budgets drawn by hook not
series (lines.test pattern), allocator rows tagged NO CONTRACT and null-safe,
composition-unavailable state, range chips re-fetch once each,
`activeTimers() === 0`, request uses exactly `MEMORY_PERF_FIELDS`.

### U7. Live Feed

`pages/live-feed/ring.ts` (bounded ring + rAF flush seam injected for tests)
+ `pages/live-feed.tsx` + phone cards + route flip. Tests: starts empty;
append 600 → capacity kept, oldest dropped; capacity 200 under a stubbed
`matchMedia` narrow match; D17 filters; pause freezes render while ring
fills; clear; verdict vocabulary closed (a frame with an unexpected verdict
renders its literal text but joins no pill class — no invention); `cached`
renders as marker only; D20 detail per kind incl. `endpoint` presence rule;
unmount releases the listener; routes.test pins `events: ['query']`;
no scheduler token in the page tree.

### U8. Styles, responsive, gallery

`styles/components.css` additions (settings grid, rail, feed cards, banner);
grid-tracks test extensions where new grids are added; dev-gallery specimens
for the banner and verdict-bordered feed card (dev-only chunk).

### U9. Verification pass and review file

Full gates (cargo + frontend); bundle table gzip+brotli recorded; browser
evidence per the Verification Contract; write
`docs/code-review/phase5/p5-09-settings-and-diagnostics-review.md` with the
Implementation Summary (naming `metadata.ts` as the hand-carried source per
the acceptance criterion) and stop, per §TASK COMPLETION.

---

## Verification Contract

**Unit (vitest)** — listed per unit above; the acceptance-named ones:
changed-keys-only bodies (U3+U4), auth redaction (U4), api-gate confirmation
(U4), banner arm/clear with no timer (U2/U4), donut sum + peak-not-a-slice
(U6), no budget wall (U6), ring bound + hidden/pause semantics (U7),
route-table pins (U1–U7).

**Browser, against a live API (dev box container/binary — the RB5009 is not
touched):**

- V1. Live Feed: DevTools WS shows `{"subscribe":["query"]}` on entry; query
  frames flow; navigate to Health → a smaller-union subscribe is never sent
  (union empties) and the socket **closes**; no query frame arrives after.
  `fahUnion()` reads `['query']` on the feed, `[]` elsewhere;
  `fahSocketState()` reads `closed` on Health/Memory.
- V2. Engine-side: the publish gate is `main.rs:671`
  (`hub.has_query_subscribers()`); its per-socket accounting is covered by the
  p5-03 fah-api tests (stats-only socket → gate `false`). Live evidence: with
  the feed closed, a second bearer socket subscribed to `stats` only receives
  zero `query` frames across ≥ 30 s of household traffic — delivery and gate
  observed together. (While any socket asks for `query` the gate is open by
  design — API.md §Events; the assertion is the all-off case.)
- V3. Settings: a boot-only edit (e.g. `dns.cache.max_entries`) → body
  contains exactly that nested key; banner appears; restart the **dev**
  container → next `/health` read on entering Settings clears it. A runtime
  edit (`history.retention_days`) applies with no banner. `GET /config`
  confirms persistence (working-agreement rule: confirm via the API, no TOML
  hand-edit).
- V4. Hidden document: on the feed, hide the tab → rendering stops
  immediately, socket closes after the 30 s grace; return → resubscribe, feed
  resumes, gap simply missing; the on-page statement matches.
- V5. Health/Memory: `fahTimers()` shows exactly the declared endpoints'
  timers on Health and zero on Memory; leaving each route returns the count
  to the incoming route's declaration within one interval.
- V6. 1400 / 900 / 390 px, both themes, all four pages; 390 px feed matches
  `MobileLiveFeed.dc.html`; 44×44 targets measured on the phone bar; no
  horizontal body scroll anywhere; dialog focus traps hold.
- V7. Bundle: build output table recorded gzip + brotli; budget gate green.

---

## Definition of Done

Every R-row demonstrated by a named test or V-item; §3 conflicts and §4b
deviations recorded in the review file; gates green (cargo fmt/clippy/test +
typecheck/test/build); bundle gzip and brotli recorded; review file written;
chat response per §TASK COMPLETION and stop. Phase-table flip to DONE follows
plan/CLAUDE.md only after gates.

---

## §13 Repository documents this task proposes to change — NOT applied

Proposals only; each waits for the owner's explicit yes, after the code is
done:

1. **CONTEXT.md** — define `permitted` (phase constraint 5's wording); revise
   §Query Log to state that FastAdHunter persists no per-query record and the
   dashboard surface is the **Live Feed** (ephemeral, bounded, starts empty),
   per the phase table row "CONTEXT.md · p5-04 or p5-09".
2. **docs/dashboard/open-questions.md** — move the "Live Feed ring size"
   bullet to §Closed (500 desktop / 200 narrow, fixed by the task).
3. **docs/project-state.md** — p5-10's rewrite, not this task's.

No API.md change: the task adds no route, field or event. No
CONFIGURATION.md change: no key is added or reclassified.

---

## Risks

- **Metadata drift** — the accepted cost of the hand-written form. Mitigated
  by source anchors per field in `metadata.ts` and the raw panel making an
  unmodelled key visible rather than absent.
- **Settings chunk weight** — the largest page of the phase (11 sections + 3
  dialogs + raw panel). Estimate: settings ~8–10 kB gzip, health ~3 kB,
  memory ~4–5 kB, live-feed ~4 kB, shared + CSS ~2–3 kB → **+21–25 kB gzip**
  on 96.4 kB, landing ~118–122 kB against 150 kB (~80 %). Headroom for p5-10
  remains ~28 kB. If the estimate is exceeded, the raw panel's renderer is
  the first candidate to simplify — never the gate.
- **Banner correctness across sessions** — in-memory arming forgets on hard
  reload (KTD3, recorded); the API offers no "pending boot-only changes"
  read, so this is the honest ceiling without inventing an endpoint.
- **rAF coalescing under stress** — a pathological event rate still costs one
  render per frame with a 500-row list; if V6 shows jank at 390 px, cap
  rendered rows below ring capacity before touching the protocol.
- **Working tree contamination** — `docs/code-review/phase2.6/…-soak-review.md`
  is modified in the tree and belongs to the 2.6 track; it stays out of this
  task's changeset (phase rule: 2.6 fixes branch from `main`).

## Deferred / Open Questions

None left for the implementer. Time-zone display (open-questions §3) does not
bind here: Memory axis captions reuse the p5-08 window-end idiom; feed rows
show browser-local clock time via `clockLabel`, matching every existing page.

## Appendix — research anchors

`config_store.rs:35` (`BOOT_KEYS`), `:108` (`apply_patch`), `:151` (`merge`,
arrays replace); `routes.rs:1332/1336/1427` (config handlers), `:124`
(health), `:164` (debug_memory); `error.rs` (`invalid value for \`{key}\``);
`events.rs:89` (`has_query_subscribers`), API.md:905–928 (subscribe
protocol); `main.rs:671` (engine gate); `ports.rs:218` (`verdict_str`);
`wire.rs:938–1097` (memory responses), API.md:318–421 (perf fields incl.
`rss_anon_bytes`/`rss_file_bytes`, `peak_rss` semantics, derived residual);
`schema/*` + `lib.rs:118` (validation bounds); PERFORMANCE.md §Budgets
(128 MB / 256 MB, MB-vs-MiB); `routes.ts:181–218` (declared p5-09 rows);
`registry.ts` (subscribe/announce), `subscriptions.ts` (union),
`socket.ts:117` (`on`); p5-08 review §Implementation Summary (chart idioms,
budget-as-hook, strategy gate); p5-07 review §Implementation Summary
(dialogs, BusyModal, 422 parsing, ownsHeader).
