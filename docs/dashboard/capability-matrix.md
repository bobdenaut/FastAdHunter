# Capability Matrix — Pi-hole UI vs FastAdHunter API

Pi-hole's web interface (`pi-hole/web`, master) is the **visual and interaction
reference** for the FastAdHunter dashboard. It is not the domain model. This
file records, feature by feature, whether the existing FAH API can back a
Pi-hole screen — and what FAH exposes that Pi-hole has no screen for.

The rule this file enforces: **Pi-hole UX → FAH API capability → FAH
implementation.** A feature with no endpoint behind it is cut, never faked.

Sources checked: [API.md](../../API.md) and
`crates/fah-api/src/routes.rs` (they agree route for route, verified
2026-08-25).

**Phase 5 adds three things to that surface**, settled in `plan/open/phase5/`
before any page is written and marked below as *(p5-03)* or *(p5-04)*: the
`/events` subscription protocol, the in-force policy on `GET /clients`, and the
session routes. Everything else in this file is what already ships.

**Re-review after Phase 3.** Phase 5 is deliberately built ahead of it. Phase 3
brings certificate machinery and per-client HTTPS interception — a screen this
file currently has no row for, and a control the Clients page will need. It is
not designed for now and comes back to this file when it lands.

Phase 4 would have forced a second re-review: cosmetic rules leaving
`rules_inactive`, and the Lists partition gaining a band. It is parked as of
2026-09-15 ([ADR-0009](../decisions/0009-phase-4-parked.md)), so that re-review
is not owed and the partition keeps the meaning it has today.

## The API surface, in full

| Route | Method | What the dashboard gets from it |
| --- | --- | --- |
| `/health` | GET | version, uptime, `ok` \| `degraded`; the one route with no auth |
| `/api/v1/stats` | GET | rolling-24h aggregate: totals, percentages, top-N tables, hourly buckets, per-policy counts |
| `/api/v1/telemetry` | GET | whole engine state: `process`, `ruleset`, `counters` (dns + http + swr + cache_cleanup), `latency`, `upstreams`, `cache`, `memory` |
| `/api/v1/history/summary` | GET | persisted hourly/daily series: queries, blocked, cache_hits, `per_type` |
| `/api/v1/history/perf` | GET | persisted per-interval samples: RSS, QPS, verdict deltas, cache, latency percentiles, memory, upstreams |
| `/api/v1/history/top` | GET | top-N over a range, `kind=blocked\|queried\|clients` |
| `/api/v1/clients` | GET | observed clients: ip, name, first/last seen, 24h queries and blocks, and *(p5-03)* the in-force policy with its assignment source |
| `/api/v1/clients/{ip}` | PUT | set or clear a client name |
| `/api/v1/clients/{ip}/policy` | GET/PUT/DELETE | the policy in force for one address, and its assignment |
| `/api/v1/cache` | GET | entries, capacity, fresh/stale/expired, hits/misses/evictions, bytes, both load percentages |
| `/api/v1/cache/clean` | POST | remove expired now; `?stale=true` also purges the serve-stale window |
| `/api/v1/lists` | GET/POST | list inventory with per-list rule partition and refresh status; add a list |
| `/api/v1/lists/{id}` | PATCH/DELETE | enable/disable, change refresh interval, remove |
| `/api/v1/lists/{id}/refresh` | POST | force one list; `202`, outcome lands in `last_status` |
| `/api/v1/lists/refresh` | POST | force every enabled list, one recompile, synchronous per-list results |
| `/api/v1/rules/user` | GET/PUT | inline personal rules as lines; PUT validates and atomically swaps |
| `/api/v1/rules/test` | POST | dry-run a verdict for a domain under a client or a named policy |
| `/api/v1/policies` | GET/POST | policy inventory, schedule timezone, `active_assignments`; create |
| `/api/v1/policies/{id}` | PATCH/DELETE | edit or remove; a `lists` change recompiles |
| `/api/v1/config` | GET/POST | effective config with secrets redacted; partial update with `applied` / `restart_required` |
| `/api/v1/config/apikey/rotate` | POST | new API key, returned once |
| `/api/v1/debug/memory` | GET | where RSS goes, per bounded structure, plus `residual_bytes` |
| `/api/v1/events` | WS | the **only** per-query feed: `query`, `stats`, `config_changed`, `list_refreshed`. *(p5-03)* a `subscribe` message narrows what a socket receives; default is every event, and only the Live Feed asks for `query` |
| session routes | *(p5-04)* | login, logout, logout-everywhere, password change — session cookie alongside the bearer key. Exact paths are fixed by `p5-03`'s reserved API.md section |

## Ships — Pi-hole screen, FAH data

| Pi-hole feature | FAH source | Notes |
| --- | --- | --- |
| Four dashboard stat tiles | `GET /stats` | Pi-hole's fourth tile is "Domains on Lists"; FAH's equivalent number is `lists.compiled_rules`, which belongs on the Ruleset card, so the fourth tile is Cache Hit % instead |
| Total-queries time chart | `GET /history/summary` | `resolution=hour\|day`, `stride` decimation; points are real readings, never averaged. DNS-only — this endpoint carries no HTTP series. The bands are **permitted** and blocked, not "allowed" (see Vocabulary) |
| Query-types donut | `history/summary.per_type` | fixed label set; zero buckets omitted |
| Top permitted / blocked domains | `stats.top_*_domains`, `history/top` | `/history/top` is an approximation by construction — the UI labels it "what dominated this range", not an exact order |
| Top clients | `stats.top_clients`, `history/top?kind=clients` | |
| Query log presentation (table shape, row density, verdict colouring) | `WS /events`, subscribed to `query` | the presentation carries over; the persistence behind it does not — see Cut. Only this screen subscribes to `query` |
| Lists management | `/lists` CRUD | per-list `rules_active_dns` / `rules_active_url` / `rules_inactive` partition is richer than Pi-hole's per-list count |
| "Update Gravity" | `POST /lists/refresh` | synchronous, best-effort, per-list `ok` \| `failed` \| `rejected` |
| Allow/deny domains | `GET\|PUT /rules/user` | shape differs — see Reshaped |
| Clients list and naming | `GET /clients`, `PUT /clients/{ip}` | FAH clients are *observed by traffic*; there is no ARP or DHCP source |
| Group assignment | `/policies`, `/clients/{ip}/policy` | shape differs — see Reshaped. The Clients **table** reads its policy column from `GET /clients` *(p5-03)*; `/clients/{ip}/policy` stays the write path and the single-address read |
| "Search Lists" (which rule blocks X) | `POST /rules/test` | strictly better: returns verdict, rule, list **and** deciding policy, and can test under a client or a hypothetical policy |
| Settings pages | `GET\|POST /config` | shape differs — see Reshaped |
| API/key settings | `POST /config/apikey/rotate` | the key is shown once; the UI says so before rotating |
| Login | *(p5-04)* | password + session cookie, no database. `auth.*` is redacted from `GET /config` and `422` on `POST /config` — one writer, the password-change route |

## Cut — no FAH backing

Nothing here is stubbed, mocked or filled with placeholder data. It is absent
from the UI.

| Pi-hole feature | Why |
| --- | --- |
| Persisted, searchable query log | FAH keeps **no per-query store**. `WS /events` is a live feed; the history endpoints are aggregate-only. This is the single largest divergence from Pi-hole. **CONTEXT.md §Query Log still defines the opposite** — "the bounded, *persisted* record of individual queries and requests", with a `domain` filter. That entry describes something that does not exist and is reconciled with **Live Feed** in this phase; the vocabulary there is binding, so two contradictory definitions cannot both stand. |
| Query-log date/time range picker, "query on-disk data" | follows from the above — there is no on-disk per-query data to range over |
| Query-log filters: upstream, reply, DNSSEC status | those fields do not exist per query. `upstream` is `null` by contract; the answering endpoint index appears only on forwarded DNS items; DNSSEC is not modelled. |
| Client Activity chart (per-client series over time) | `/history/*` holds no per-client series. Per-client numbers exist only as 24h totals on `/clients` and as top-N rankings. |
| Disable blocking (indefinitely / 10s / 30s / 5m / custom) | no endpoint disables the Rule Engine |
| DHCP settings and leases | FAH is not a DHCP server |
| Local DNS records | FAH does not author records |
| Network / ARP table | no such data source |
| Interfaces | not exposed |
| Privacy levels | FAH has no query-anonymisation setting |
| Teleporter (backup / restore) | no import-export endpoint. `GET /config` is read-only and redacted, so it cannot round-trip. |
| Tail log files (`pihole.log`, `FTL.log`, `webserver.log`) | no log-reading endpoint |
| Pi-hole diagnosis / messages | no message store. The Diagnostics page covers the same *need* from real signals instead: `/health` status, per-list `last_error`, upstream `consecutive_failures`, `counters.events_dropped`. |
| Donate | Pi-hole-specific |
| Groups as a first-class many-to-many entity | FAH has no groups — see Reshaped |

## Reshaped — same need, different FAH semantics

Pi-hole's information architecture is not reproduced where the underlying model
differs. Copying the screen would promise behaviour the API does not have.

| Pi-hole | FAH | Why the shape changes |
| --- | --- | --- |
| **Query Log** as the second nav item and the record of truth | **Live Feed**, under Diagnostics | naming it a log promises retained history. It is a debugging tail: it starts empty on page load, holds a bounded client-side ring, and says so. Slow consumers are disconnected by design. |
| **Groups** + Clients + Domains + Lists, four pages over a membership matrix | **Policies** page, assignment edited inline on **Clients** | a FAH policy is a named list subset plus optional schedule, and an address carries at most one assignment. There is no many-to-many to render. |
| **Domains** as CRUD rows | **Custom Rules** as a validated text document | `/rules/user` is lines in, lines out, PUT-validated and atomically swapped. Row CRUD would invent a per-rule identity and a per-rule write that do not exist. Per-line `422` messages anchor to the editor's lines. |
| **Upstream Servers** pie (share of queries) | **Upstreams** health view | per-query attribution is deliberately not carried. What FAH has is endpoint health: attempts, failures, `consecutive_failures`, TLS handshakes, and the adaptive strategy's state. |
| **Settings** split DNS / DHCP / privacy / API | **Settings** grouped by config section, each field tagged live-apply or restart-required | FAH's real axis is mutability class, not subsystem. Most options are boot-only. `rules.lists` and `policies` are `422` here and are absent from the form — they belong to their own pages. |
| **Tools** menu (gravity, search, tail, diagnosis, network) | refresh is a button on **Lists**; search is the **Rule Tester** page; the rest are cut | five unrelated things sharing a menu is Pi-hole's history, not an organising principle |
| DNS-only framing throughout | DNS and HTTP shown as two pipelines | FAH answers DNS questions *and* proxies HTTP. `counters.dns` and `counters.http` are deliberately separate because "queries" has meant "DNS questions answered" since p1-08. |

## FAH-only — surfaced in the same visual language

Pi-hole has no screen for these. They get FAH pages built from the same tiles,
cards and tables, so the interface reads as one system.

| Capability | Source | Screen |
| --- | --- | --- |
| HTTP pipeline counters — pass / allow / block / refused / `response_bytes` | `telemetry.counters.http` | second tile row on Dashboard; `refused` is the only signal of a LAN client probing and gets its own tile |
| Cache lifetime stages — fresh / stale / expired, both load percentages | `GET /cache` | **Cache** page; the higher of `load_percent` / `byte_load_percent` is marked as the bound about to evict |
| Manual cache clean, with the stale-window choice | `POST /cache/clean` | explicit toggle, worded as insurance being purged, plus the fact that RSS does not drop by `freed_bytes` |
| Stale-while-revalidate counters | `telemetry.counters.swr` | Cache page |
| Cache-cleanup run history | `telemetry.counters.cache_cleanup` | Cache page |
| Latency percentiles per class — block / cache_hit / forward, p50 and p99 | `history/perf.latency` | **Performance** page |
| QPS and RSS series | `history/perf` | Performance page |
| Memory breakdown per bounded structure plus `residual_bytes` | `GET /debug/memory` | Diagnostics; the two `allocator_committed_*` fields are labelled as promising nothing |
| Ruleset compile stats — rules, `duplicates_removed`, `compile_duration_seconds` | `telemetry.ruleset` | Dashboard card and Lists header |
| Answer outcomes — `servfail_synthesized`, `servfail_relayed`, `refused_relayed` | `telemetry.counters.dns.answers` | Diagnostics |
| Shed events | `telemetry.counters.events_dropped` | Diagnostics; covers both pipelines, one number |
| Adaptive upstream strategy state | `telemetry.upstreams`, `/health` | **Upstreams**; `degraded` is explained in place — cache hits and serve-stale keep answering, so it is not "down" |
| Policy schedules with timezone and live `active_assignments` | `GET /policies` | **Policies** |
| Verdict dry-run under a client *or* a hypothetical policy | `POST /rules/test` | **Rule Tester** |
| Per-policy traffic split | `stats.policies` | Policies page; counts both pipelines, unlike the domain tables |

## Vocabulary

The UI uses [CONTEXT.md](../../CONTEXT.md) terms, not Pi-hole's.

| Say | Not |
| --- | --- |
| pass / allow / block | OK / forwarded / blocked |
| permitted (the derived `queries − blocked` band) | allowed |
| cached (a flag on an event) | "cached" as a status value |
| stale (inside the RFC 8767 serve-stale window) | stale as "old" |
| answering endpoint (an index into the configured servers) | upstream server, where the index is meant |
| policy | group |
| rule list | gravity, blocklist |
| compiled rules | domains on lists |

**`allow` and `permitted` are not synonyms and must never be swapped.** `allow`
is the explicit exception verdict the API counts — `counters.dns.allow`,
`history/perf.allowed_delta` — four figures against six-figure traffic.
`permitted` is `queries − blocked`, which no endpoint carries and the UI derives
for charts. Labelling the derived band "allowed" claims a measurement the engine
never took; relabelling a real `allow` counter "permitted" throws away the
distinction CONTEXT.md §Verdict exists to make. `permitted` is added to
CONTEXT.md in this phase, since the vocabulary there is binding.
