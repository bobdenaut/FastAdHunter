# p5-10 — Phase 5 Verification — Review

## Implementation Summary

Nothing was built in this task. It measures the shipped phase-5 artefact and
records the evidence. Per the task file's **§Execution split**, this file holds
**Stage A only** — everything a dev box can answer while the p2.6-11 soak still
owns the RB5009. Stage B rows stay open and are marked `AWAITING SOAK`.

**The RB5009 was not contacted in any form.** Every figure below comes from a
local container on the dev box or from a local build.

| Stage A item | Result |
| ------------ | ------ |
| End-to-end tests (sign in, 13 pages, one mutation per mutating page, sign out) | **26/26 pass**, with a negative control proving the page assertions have teeth |
| Route-ordering regression | **pass** — unknown API paths answer JSON, never the shell |
| Bundle, gzip + brotli + chunks + uPlot + login path | **128,730 B gzip (83.8 % of budget)**, 114,097 B brotli |
| Image size, phase delta, no Node / no toolchain | **arm64 14.07 MiB rootfs**, amd64 16.55 MiB; zero Node/toolchain paths |
| Page-by-page figure trace | see §6 |
| Route-scoped fetching table | see §7 |
| Exhaustive UI audit — every control, dialog, save and link | **§8** — 128 controls swept, 113 deep checks, 69 links, 0 render faults |
| Socket-load run | see §9 |
| Mobile pass | see §10 — emulated complete; the real-phone leg needs the owner |
| Findings | **1 Major, 9 Minor, 1 Nit, 2 observations** — §11. Three fixed in §12 |

---

## 1 · Corpus, workload, device

Root CLAUDE.md rule 19 — every figure below is scoped by this block.

| | |
| --- | --- |
| **Device** | dev box, `win32` host, Docker Desktop 29.7.2, linux containers |
| **Artefact under test** | `fah:p510-amd64`, built from working tree at `142f387` (branch `phase5-10`), version `0.2.20` |
| **Baseline artefact** | `fah:base-amd64`, built in the same session from `59601c0` — the commit the phase-5 branch chain forked from, i.e. a real pre-change checkout, not a stored figure |
| **Corpus** | `oisd-basic` (`https://small.oisd.nl`), **59,207 compiled rules**, 0 duplicates, 0 parse errors, compile 0.03 s |
| **Workload** | two container clients (`172.17.0.4`, `172.17.0.5`) driving a fixed 11-domain DNS mix — 6 blocked by the corpus, 5 permitted — at ~3 qps sustained for the whole session; ~10.3 k queries/hour, 46.6 % blocked |
| **Upstreams** | `192.168.65.7` (Docker Desktop resolver) primary, `192.168.65.254` secondary, `strategy = "fallback"` — the shipped defaults `1.1.1.1` / `9.9.9.9` are unreachable from a container on this host, and that is the only change made to the generated config |
| **Browser** | Chromium 149.0.7827.55, driven by `playwright-core` 1.62.0-alpha, `ignoreHTTPSErrors` (self-signed API certificate) |

The blocked share (46.6 %) is an artefact of the synthetic domain mix and is
**not** a household figure — see
[measurement-traps.md](../../measurement-traps.md) §Traffic and rates.

---

## 2 · Quality gates

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | **1,196 tests, 0 failures**, 44 suites |
| `npm run typecheck` (`tsc --noEmit`) | clean |
| `npm run test` (vitest) | **54 files, 928 tests, 0 failures** |
| `npm run build` incl. size gate | pass — 83.8 % of the 150 KB gzip budget |

`crates/fah-api/tests/request_coverage.rs` is in that run and green:
`every_route_the_api_serves_has_a_request_in_the_suite` and
`every_documented_exclusion_still_names_a_real_route`, 2/2.

---

## 3 · Bundle

`npm run build` on the working tree; `scripts/postbuild.mjs` computes gzip at
level 9 and brotli at quality 11, and writes the `.gz` / `.br` siblings the
handler serves.

| | raw | gzip | brotli |
| --- | ---: | ---: | ---: |
| **TOTAL (39 emitted files)** | 371,491 | **128,730** | **114,097** |
| Budget (gzip, enforced by the build) | — | 153,600 | — |
| Headroom | — | 24,870 B (16.2 %) | — |

Measured on the tree **after** the three fixes in §12. Before them the total was
128,767 B gzip / 114,138 B brotli — the fixes are net **−37 B** gzip, because
the reworded Live Feed sentence is shorter than the CSS the two target rules
added.

**Largest contributors, gzip:**

| Chunk | raw | gzip | brotli | What it is |
| --- | ---: | ---: | ---: | --- |
| `uPlot.esm-*.js` | 50,996 | **21,997** | 19,884 | the chart library, alone |
| `style-*.css` | 60,591 | 12,156 | 10,772 | the single stylesheet (`cssCodeSplit: false`) |
| `settings-*.js` | 33,230 | 10,919 | 9,601 | Settings |
| `index-*.js` | 30,664 | 9,920 | 8,836 | entry: shell, router, client, socket |
| `diagnostics-memory-*.js` | 27,100 | 9,545 | 8,598 | Memory |
| `dashboard-*.js` | 17,590 | 6,362 | 5,720 | Dashboard |
| `icon-*.js` | 15,199 | 6,338 | 5,753 | icon set |
| `lists-*.js` | 14,967 | 4,765 | 4,189 | Lists |
| `policies-*.js` | 14,733 | 4,838 | 4,290 | Policies |
| everything else (30 files) | 106,197 | 41,927 | 36,495 | per-route chunks and shared leaves |

**uPlot costs 21,997 B gzip / 19,884 B brotli — 17.1 % of the shipped gzip
total**, and it is the single largest item in the bundle. It is loaded by the
four charting routes only.

**Login path.** `index.html` + `index-*.js` + `login-*.js` + `style-*.css` =
**23,640 B gzip**. The login chunk's only static imports are `icon-*.js` and
`index-*.js`; **no chart chunk is on the login path**, asserted two ways:
`postbuild.mjs` `chartSplitViolations()` greps every login-path asset for
`uplot` and for uPlot's vendor-only selectors, and the live network log for
`/login` shows no `uPlot.esm-*.js` request.

**No web font ships.** `dist/` contains no `.woff`/`.woff2`/`.ttf`/`.otf`, no
`@font-face` rule, and the stack is
`system-ui, -apple-system, "Segoe UI", Roboto, …` / `ui-monospace, …`. The
postbuild's external-URL scan (which would catch a Google Fonts link) passes.

---

## 4 · Image

Two images built in the same session, `linux/amd64`, from the pre-change commit
and from the working tree. The arm64 image — the one that ships — was built
from the working tree as well.

Sizes are the **rootfs tar** (`docker export`), which counts what is actually in
the image; `docker images` SIZE is quoted beside it because that is the measure
`p5-01` used.

| Figure | base `59601c0` (amd64) | p5-10 (amd64) | Δ | p5-10 (**arm64**, ships) |
| --- | ---: | ---: | ---: | ---: |
| rootfs tar | 16,094,720 B (15.35 MiB) | 17,355,776 B (16.55 MiB) | **+1,261,056 B (+7.8 %)** | **14,753,792 B (14.07 MiB)** |
| `docker images` SIZE | 24.9 MB | 27 MB | +2.1 MB | — (foreign arch, not unpacked) |
| `/fastadhunter` | 12,970,400 B | 13,511,264 B | +540,864 B (+4.2 %) | 10,909,696 B |
| `/web` | absent | 614,181 B in 137 files | +614,181 B | 614,181 B in 137 files |
| `tzdata` layer (base image) | 4.24 MB | 4.24 MB | 0 | 4.24 MB |

Rebuilt after the §12 fixes; `/web` moved 614,035 → 614,181 B and the rootfs
tar did not move at all, the difference fitting inside existing 512-byte block
padding. Local `dist/` is byte-identical to the image's `/web` (614,181 B in
137 files), which is what makes the §12 re-verification against a container
equivalent to testing the image.

**Budget: ≤ 30 MB. The shipping arm64 image is 14.07 MiB (14.75 MB) — 49 % of
budget.** The phase's own delta is the binary's +540,864 B plus `/web`'s
614,181 B; the tar's +1,261,056 B is those two plus tar block padding over 139
new entries.

`/web` is 137 files because every text asset ships with its `.gz` and `.br`
sibling; the uncompressed page weight is the 371,491 B in §3.

**Runtime image proven free of Node and build tooling.** The full 1,585-entry
rootfs listing was searched case-insensitively for
`node|npm|node_modules|\.map$|cargo|rustc|gcc|python|\.a$|\.o$` — **0 matches**
on both amd64 and arm64. No source map is emitted by the build and none reaches
the image.

**Observation, not a finding:** 4.24 MB of the image — 29 % of the arm64 rootfs
— is `/usr/share/zoneinfo` (1,308 entries) contributed by
`gcr.io/distroless/static-debian12:nonroot`. FastAdHunter never reads it. See
finding **S3**.

---

## 5 · End-to-end and route ordering

### 5.1 · End-to-end, against the running container

One Chromium session per run: sign in with a wrong password, sign in, walk all
thirteen screens **by clicking the sidebar** (SPA navigation, not reloads), one
mutation per mutating page with a revert, sign out.

| # | Check | Result |
| - | ----- | ------ |
| 1 | `/` unauthenticated redirects to `/login` | pass |
| 2 | wrong password rejected, stays on `/login`, renders "That password is not right." | pass |
| 3 | session cookie is `__Host-fah_session`, `Secure`, `HttpOnly`, `SameSite=Strict` | pass |
| 4–16 | each of the thirteen screens renders **live data**, not a shell | 13/13 pass |
| 17 | Lists — `PATCH /api/v1/lists/oisd-basic` `200`, row redraws `24 h → 12 h`, reverted | pass |
| 18 | Custom Rules — `PUT /api/v1/rules/user` `200`, editor reports 2 lines, reverted to empty | pass |
| 19 | Policies — `POST /api/v1/policies` `201` behind its recompile confirmation, card appears | pass |
| 20 | Policies — `DELETE /api/v1/policies/e2e-probe` `204` behind confirmation, card gone | pass |
| 21 | Clients — `PUT /api/v1/clients/172.17.0.4` `200`, NAME column redraws, survives leaving and returning, reverted | pass |
| 22 | Rule Tester — `POST /api/v1/rules/test` `200`, verdict **BLOCK**, rule `\|\|analytics.google.com^`, list `oisd-basic`, policy `default` | pass |
| 23 | Cache — `POST /api/v1/cache/clean` `200`, result card names removed / freed / before→after / took | pass |
| 24 | Settings — `POST /api/v1/config` `200` on a live-apply key (`rules.refresh_hours_default` 24→48), answered "Applied live", reverted | pass |
| 25 | Sign out — lands on `/login`, `__Host-` cookie gone, a subsequent `GET /api/v1/telemetry` answers `401` | pass |
| 26 | no uncaught page error across the whole run | pass |

**26/26.**

### 5.2 · The page assertions are not vacuous — negative control

The same thirteen predicates were re-run with **every API read aborted at the
transport** (`/api/v1/**` and `/health`) **and the events socket cut**
(`routeWebSocket` closing it). A predicate that still passes is testing nothing.

| Result | Count |
| --- | --- |
| predicate correctly failed with no data | **12 / 13** |
| predicate still passed | 1 — `/rule-tester` |

`/rule-tester` cannot be failed this way and this is correct behaviour, not a
weak assertion: the only entry always present in its policy selector is
`default`, which is **implicit and client-side** — no endpoint carries it. The
page's real data path is the test result itself, which check 22 asserts against
a live `POST /rules/test`.

Two error states were observed while the control ran and are worth recording as
good behaviour: `/rules` renders **"The API did not answer — GET
/api/v1/rules/user did not reach the server"** with the indicator on
`API not answering`, and the Dashboard's HTTP tiles fall to `—` while its DNS
tiles keep updating from the `stats` socket frames.

### 5.3 · Route ordering, re-asserted

`crates/fah-api/tests/api.rs` keeps three tests from `p5-01`, all green in this
run's `cargo test`:

| Test | Asserts |
| --- | --- |
| `an_unknown_route_is_a_json_not_found` | `/api/v1/nope` → `404` `{"error":{"code":"not_found"}}`; anonymous → `401`, **no `vary` header**, i.e. not answered by the static service |
| `every_path_under_api_is_json_never_the_shell` | `/api`, `/api/`, `/api/v2/stats`, `/api/v1/nope` — all JSON `404`, all `401` anonymously, none carries `vary` |
| `the_static_paths_are_outside_the_api_key_boundary` | `/`, `/assets/…`, `/lists/oisd-basic` — never `401`, always `vary: accept-encoding`, never `application/json`, body carries no `"error"` |

Confirmed live against the running container as well:

| Request | Status | Content-Type | `vary` |
| --- | --- | --- | --- |
| `GET /api/v1/nope` (with key) | `404` | `application/json` | absent |
| `GET /api/v1/nope` (anonymous) | `401` | `application/json` | absent |
| `GET /settings` (deep link) | `200` | `text/html` | `accept-encoding` |

---

## 6 · Page-by-page figure trace

Every figure rendered anywhere in the UI falls into exactly **three**
categories. Anything outside them would be a defect; none was found.

| Category | Where it is defined | How it is recognised |
| --- | --- | --- |
| **API field** | the endpoint named in the page's own header line | read straight from the response |
| **Derived** | `src/derive.ts`, one exported function per catalogued row (`R…`/`E…` ids from the p5-06/07/08 plans) | never invented at the call site |
| **Documentation-sourced constant** | `pages/performance/budgets.ts`, `pages/memory/budgets.ts` | drawn as a marker or a chip, carrying its source and its device |

The third category is the one worth stating out loud: `20 k+ sustained capacity`,
`BUDGET < 1 ms`, `128 MB steady-state budget` and `256 MB hard-ceiling budget`
are **not** measurements this build took. They come from
[PERFORMANCE.md §Budgets](../../../PERFORMANCE.md), the modules that hold them
say so, and each is rendered with its provenance beside it.

| Page | Figures | Source |
| --- | --- | --- |
| **Dashboard** | Total queries · Queries blocked · Percentage blocked · Cache hit rate | `stats.queries_total` · `.blocked_total` · `.blocked_percent` · `.cache_hit_percent` |
| | HTTP requests | **derived** `counters.http.pass + allow + block` — the API carries no HTTP total |
| | HTTP blocked · refused footer | `counters.http.block` · `.refused` |
| | Compiled rules · Uptime · status footer | `telemetry.ruleset.rules` · `telemetry.process.uptime_seconds` · `health.status` |
| | active-clients footer | `clients.items.length` |
| | Queries over time (permitted / blocked bands) | `history/summary.items[].queries`, `.blocked`; **permitted is derived** `queries − blocked` and the legend says `permitted`, never `allowed` |
| | Query types donut | `history/summary.items[].per_type`, summed over the range (`derive.queryTypeSlices`, `derive.sliceShare`) |
| | Upstream health card | `telemetry.upstreams[].attempts`/`.failures` (`derive.upstreamBar`); `.address` from the same array |
| | Top queried / blocked / clients | `stats.top_queried_domains`, `.top_blocked_domains`, `.top_clients` |
| | Cache state card | `cache.entries`/`.capacity`/`.fresh`/`.stale`/`.expired`/`.hits`/`.evictions`/`.load_percent`/`.byte_load_percent`; `free` is **derived** `capacity − entries` (`derive.freeEntries`) |
| | Ruleset card | `telemetry.ruleset.rules`/`.duplicates_removed`/`.compile_duration_seconds`; enabled lists from `lists.items` |
| **Lists** | header stats | `lists.compiled_rules` · `.duplicates_removed` · `telemetry.ruleset.compile_duration_seconds` · enabled/configured counted from `lists.items` |
| | per-row | `items[].id`/`.url`/`.enabled`/`.refresh_hours`/`.last_refresh`/`.last_status`/`.last_error`/`.rules_active_dns`/`.rules_active_url`/`.rules_inactive`/`.parse_errors`/`.rules_total` |
| **Custom Rules** | line count, editor body | `rules/user.rules` |
| **Policies** | policies · ceiling, assignments in force, assignments configured, timezone | `policies.items.length` (+ the implicit `default`), `.active_assignments`, `.items[].assignments`, `.timezone` |
| | per-policy traffic, 24 h | `stats.policies[].queries` / `.blocked` |
| | lists chip | `policies.items[].lists` resolved against `lists.items` |
| **Clients** | address, name, queries 24 h, blocked, last seen | `clients.items[].ip`/`.name`/`.queries_24h`/`.blocked_24h`/`.last_seen` |
| | blocked share bar | **derived** `blocked_24h / queries_24h` (`derive.blockedPercent`) |
| | policy in force + assignment source | `clients.items[].policy` plus `policies` for which selector matched |
| **Rule Tester** | verdict, rule, list, deciding policy | `rules/test` response, verbatim |
| **Cache** | fresh · stale · expired · free · entries · capacity · bytes · max_bytes · both load percentages · hits · misses · evictions | `/cache` fields; `free` and `lookups` are **derived** (`derive.freeEntries`, `derive.cacheLookups`), hit rate is `derive.cacheHitRate` |
| | SWR counters | `telemetry.counters.swr.*` |
| | Background cleanup | `telemetry.counters.cache_cleanup.*` |
| | Clean result card | `POST /cache/clean` response |
| **Performance** | three stage tiles, latency chart | `history/perf.items[].latency.{block,cache_hit,forward}_{p50,p99}` |
| | qps latest / busiest | `history/perf.items[].qps` (`derive.qpsStats`) |
| | verdicts chart | `.blocked_delta`, `.allowed_delta`, and `pass` **derived** as `queries_delta − blocked_delta − allowed_delta` (`derive.passDelta`, the definition `crates/fah-model/src/perf.rs` documents) |
| | `BUDGET < 1 ms`, `20 k+` | **PERFORMANCE.md constants**, `pages/performance/budgets.ts` |
| **Upstreams** | index, address, protocol, family | `telemetry.upstreams[]`, verbatim |
| | attempts · failures · consecutive · TLS handshakes · penalties · penalized for · probes · probe successes | `telemetry.upstreams[].*`, verbatim |
| | failure-run histogram | `.failure_runs`, normalised to the row's own largest bucket (`derive.failureRunShares`); counts printed verbatim |
| **Settings** | every field value | `config.*`, one field per key; bounds, enums and the LIVE/RESTART tags come from the schema and CONFIGURATION.md, not from the response |
| **Health** | status, uptime, version | `/health` |
| | answer outcomes | `telemetry.counters.dns.answers.*` |
| | backpressure | `telemetry.counters.events_dropped`, `.http.refused`, `.swr.dropped`, `.swr.failed` |
| | rule lists needing attention | `lists.items[].last_status` (`derive.listsNeedingAttention`) |
| | engine card | `telemetry.ruleset.*`, `telemetry.memory.process_rss`, `.process_peak_rss` |
| **Memory** | RSS, peak, residual, allocator committed + peak, ruleset, cache, stats, cache entries, page faults | `/debug/memory` fields, verbatim |
| | composition stack, 24 h trend, fault rate | `history/perf.items[].{rss_bytes,peak_rss,memory,minor_page_faults}` (`derive.stackedMemory`, `derive.faultRate`, `derive.windowTrend`) |
| | `128 MB` / `256 MB` markers, headroom | **PERFORMANCE.md constants**, `pages/memory/budgets.ts` |
| **Live Feed** | time · pipe · client · domain/path · verdict · rule · list · detail · ms | `WS /events` `query` frame fields `ts`/`kind`/`client`/`client_name`/`domain`/`path`/`verdict`/`rule`/`list`/`qtype`/`cached`/`endpoint`/`duration_ms` |
| | `N / M rows held`, page counter | client-side ring state, labelled as such |

**Vocabulary, checked against the rendered DOM.** `permitted` appears only as
the Dashboard chart band; `allow` appears only where a real `allow` counter is
drawn (Performance verdicts chart, Cache/HTTP counters). Neither word is used
for the other anywhere in the shipped bundle.

### 6.1 · Live equality check

The trace above is read from the source. This is the same claim measured: for a
figure on every page, what the DOM renders is compared against **the exact
payload that page received** — response bodies captured off the page's own
requests, not re-read afterwards, so counter drift cannot hide a mismatch.

**33 / 33 matched.**

| Page | Figure | Rendered | Source |
| --- | --- | ---: | --- |
| Dashboard | Total queries | 36,308 | `stats.queries_total` (socket frame) |
| | Queries blocked | 16,868 | `stats.blocked_total` |
| | Percentage blocked | 46.5 % | `stats.blocked_percent` |
| | Cache hit rate | 53.4 % | `stats.cache_hit_percent` |
| | Compiled rules | 59,207 | `telemetry.ruleset.rules` |
| | HTTP requests | 0 | derived `http.pass + allow + block` |
| | active clients | 2 | `clients.items.length` |
| | Cache state entries | 22 | `cache.entries` |
| Lists | compiled rules | 59,207 | `lists.compiled_rules` |
| | row dns rules | 59,207 | `items[0].rules_active_dns` |
| | row total | 59,207 | `items[0].rules_total` |
| Custom Rules | line count | 0 | `rules/user.rules`, empty document |
| Policies | assignments in force | 0 | `policies.active_assignments` |
| | schedule timezone | UTC | `policies.timezone` |
| | default traffic, 24 h | 36,318 | `stats.policies[default].queries` |
| Clients | `172.17.0.5` queries 24 h | 12,124 | `clients.items[0].queries_24h` |
| Cache | fresh | 12 | `cache.fresh` |
| | hits | 19,432 | `cache.hits` |
| | lookups | 19,474 | derived `hits + misses` |
| | SWR enqueued | 652 | `telemetry.counters.swr.enqueued` |
| Performance | latest sample qps | 3.0 | `history/perf` last item `.qps` |
| Upstreams | endpoint 0 attempts | 698 | `telemetry.upstreams[0].attempts` |
| | endpoint 0 address | 192.168.65.7 | `telemetry.upstreams[0].address` |
| Settings | `dns.cache.max_entries` | 10000 | `config.dns.cache.max_entries` |
| | `schedule.timezone` | UTC | `config.schedule.timezone` |
| Health | status | ok | `health.status` |
| | compiled rules | 59,207 | `telemetry.ruleset.rules` |
| | events dropped | 0 | `telemetry.counters.events_dropped` |
| Memory | RSS · resident | 148.1 MiB | `debug/memory.process_rss` |
| | peak RSS | 175.4 MiB | `debug/memory.process_peak_rss` |
| | residual | 146.3 MiB | `debug/memory.residual_bytes` |
| | cache entries | 22 | `debug/memory.cache_entries` |
| Live Feed | first row | `19:25 · DNS · 172.17.0.5 · mozilla.org · PASS · — · — · A cached · 0.0` | `WS /events` `query` frame fields |

**One correction the measurement forced.** The Dashboard's four DNS tiles do
**not** match the REST `/api/v1/stats` body once the page has been open a few
seconds — they are ahead of it. They are fed by `stats` **socket frames**, and
the tile equals the frame either side of the DOM read. That is the phase rule
*nothing polls what `WS /events` already pushes*, visible in the data: the first
comparison against the stale REST body read 35,924 on screen against 35,912 in
the body, and the tile was right.

`telemetry.upstreams` is an **array** carrying `address` per endpoint, so the
Upstreams page's addresses come from telemetry; the `GET /api/v1/config` that
page also makes is a one-shot read for the fields telemetry does not carry.

---

## 7 · Route-scoped fetching, measured

Method: one Chromium session, signed in once, **parked ten minutes on each of the
thirteen screens in turn**, navigating by clicking the sidebar. Two independent
instruments, as the task file requires — the browser's own request log on one
side, and the server's connection count on the other, counted inside the API
container's network namespace with
`docker run --rm --network container:fah-p510 busybox netstat -tn`, sampled once
a minute.

Entry reads (what a route fetches once on mount) are excluded from the dwell
column; the dwell column is what the route costs **while nobody touches it**.

Dwell length is 600 s on the five screens that declare a polled endpoint — two
full 300 s ticks, the shortest window in which a cadence can be told from an
absence — and 180 s on the eight that declare none, where the claim is only that
nothing happens.

| Screen | dwell | requests during the dwell | server connections | declared in `routes.ts` |
| --- | ---: | --- | ---: | --- |
| `/` Dashboard | 602 s | `/health` ×10, `/telemetry` ×2, `/cache` ×2, `/clients` ×2, `/lists` ×2 | **2** | 5 endpoints, `stats` |
| `/lists` | 602 s | `/lists` ×2 | **2** | `lists`, `list_refreshed` |
| `/rules` | 181 s | **none** | **1** | none |
| `/policies` | 181 s | **none** | **1** | none |
| `/clients` | 181 s | **none** | **1** | none |
| `/rule-tester` | 181 s | **none** | **1** | none |
| `/cache` | 602 s | `/cache` ×2, `/telemetry` ×2 | **1** | `cache`, `telemetry` |
| `/performance` | 181 s | **none** | **1** | none |
| `/upstreams` | 602 s | `/health` ×10, `/telemetry` ×2 | **1** | `telemetry`, `health` |
| `/settings` | 181 s | **none** | **2** | none, `config_changed` |
| `/diagnostics/health` | 602 s | `/health` ×10, `/telemetry` ×2, `/lists` ×2 | **1** | `health`, `telemetry`, `lists` |
| `/diagnostics/memory` | 181 s | **none** | **1** | none |
| `/diagnostics/live-feed` | 181 s | **none** | **2** | none, `query` |

Against the task file's checklist:

| Check | Expected | Measured |
| --- | --- | --- |
| Sit on each of the thirteen screens | only that screen's own reads, nothing it does not need | **holds on all thirteen.** Every request in the dwell column is an endpoint that screen declares. No screen fetched anything else, once |
| Navigate away from each screen | its traffic reaches zero within one refresh interval | **holds.** After leaving the Live Feed for an endpoint-free route: **0 requests in 6 minutes**, twice the longest refresh interval |
| Active route on any of the nine event-free screens | WebSocket closed, confirmed by server connection count | **holds.** **1** connection on all nine; **2** on exactly the four that declare an event type |
| Enter Live Feed, then leave | `query` subscribed then released | **holds.** 2 connections on the feed, back to **1** after leaving, and §9 shows the subscribe/release frames |
| Visit all thirteen screens in sequence | client memory does not grow with the number visited | **holds, with the caveat below.** `usedJSHeapSize` across the thirteen: 3.50, 3.94, 4.31, 4.01, 4.67, 5.30, 4.76, 5.17, 5.68, 5.18, 5.77 MB (plus 3.96 and 4.27 for `/` and `/lists`). It rises ~2 MB end to end and is **not monotonic** — it falls at screens 7 and 10 — so this is allocation the collector has not yet reclaimed, not retention per screen. `usedJSHeapSize` without a forced collection cannot prove more than that; §9 is the stronger instrument, holding the heap in a 0.6 MB band over ten minutes on the busiest screen |

**The cadence is exact.** Ten `/health` reads in 602 s is the 60 s interval; two
reads is the 300 s interval. No screen shows a double timer, and no screen shows
a timer that outlived it.

**The Dashboard is the sharpest row.** It holds a socket and still makes **zero**
`/stats` requests in ten minutes, because `stats` arrives on that socket — the
phase's *nothing polls what `WS /events` already pushes* rule, measured rather
than asserted.

**Connection counts match `router/routes.ts` exactly.** Two connections on
Dashboard (`stats`), Lists (`list_refreshed`), Settings (`config_changed`) and
Live Feed (`query`); one on the other nine. The second connection is the
WebSocket; the first is the HTTP keep-alive every screen holds.

Two limits of this run, stated rather than glossed. The eight endpoint-free
screens were held for **180 s, not 600 s** — enough to show that a screen
declaring no endpoint issues nothing, not enough to have caught a timer with a
period between 3 and 10 minutes, and no such period exists in
`constants.ts`. And `/` and `/lists` come from an earlier invocation of the same
harness against the same container and the same build; the run was restarted
after those two screens, so their rows are from a different sitting.

---

## 8 · Exhaustive UI audit

Beyond the scoped e2e walk, every interactive control on every screen was
enumerated and exercised, every dialog filled and submitted, every in-page link
followed, and every screen probed for render faults and load time. Four
instruments, all against the same running container.

### 8.1 · Control sweep — every control on every screen

170 controls enumerated; 128 exercised (42 skipped as not visible at 1400 px,
disabled at rest, or — for `Sign out` — because they would end the audit's own
session). Each click, toggle, select change and field edit recorded the request
it fired, the dialog it opened and the page text it changed.

**20 of the exercised controls reported an error, and all 20 are the driver's
fault, not the app's** — the sweep re-finds controls by index into a live DOM,
and Settings and the Live Feed re-render enough that an index stops naming the
same element. Two consequences worth stating plainly:

- The sweep's Settings attribution is **not evidence**. On that page an index
  drift clicked *Save changes* while a field was dirty, which wrote a config
  with one upstream removed. It was restored immediately and the running engine
  never used it (`dns.upstreams` is a boot-only key). Settings was re-tested
  with explicit selectors — §8.2.
- Three checkbox clicks timed out. This is **correct app behaviour**: the pill
  switch draws a 44 × 44 pseudo-element over a visually-hidden input
  (`components.css:1404-1435`), so the pointer lands on the target, not on the
  input. Measured properly in §8.4.

### 8.2 · Deep pass — dialogs filled, saves submitted, links followed

113 checks, **106 pass**. Every failure is listed below with what it turned out
to be.

| Area | Result |
| --- | --- |
| **links** | **69/69.** Every in-page link on Dashboard, Lists, Cache, Health, Policies and Upstreams — including `watch the live feed`, `inspect the cache`, `manage lists`, `Open Upstreams →`, `Open Lists →`, `Open Memory →` — resolves to the route its `href` names |
| **rule-tester** | **9/9.** BLOCK/PASS verdicts correct for three domain–client pairs; all five query types answer; policy mode answers under the named policy; the session ring lists prior tests |
| **live-feed** | filters, pause, three page sizes, Older/Newer paging all correct; `Clear` correct (see below) |
| **settings** | restart-class key answers `restart_required` and arms the banner; `api.tls` warns and sends nothing until confirmed; a wrong current password is refused `401`; rotate and sign-out-everywhere both warn before acting |
| **lists** | add (valid source), remove-with-confirmation, edit-and-save, disable→recompile→re-enable all correct |
| **policies** | id validation, create behind the recompile warning, assign a client, delete and release |
| **clients** | search filters the table; the change-policy editor opens in place |
| **cache** | the stale toggle reaches the request; result card reports `20 stale removed, 32 → 12` |

Failures, resolved:

| Reported failure | What it actually is |
| --- | --- |
| `live-feed/clear` | **not a defect.** Clear emptied a 116-row ring; the assertion read it 1.5 s later, by which time 6 new events had arrived at ~4 events/s |
| `policies/assign` ×2 | **driver fault.** The script filled `Window end` instead of `Client selector`. Re-driven correctly: assignment saves (`201`), `active_assignments` goes to 1, the Clients row reads `assign-probe · no schedule — always · in force`, and delete releases it |
| `rules/save` invalid line | **a real defect** — finding **S6** |
| `lists/add` empty form, `lists/edit` 0 hours | **real, minor** — finding **S8** |
| `lists/add` non-URL source | **a real defect** — finding **S7** |

### 8.3 · Render correctness — all thirteen screens

Each screen probed for placeholder leakage (`undefined`, `NaN`,
`[object Object]`, `Infinity`, a literal `null`), unresolved sprite references,
zero-sized SVGs, text clipped by its own box, elements still marked
`aria-busy`, and chart canvases with nothing painted on them.

| | |
| --- | --- |
| Screens probed | 13 |
| Render faults found | **0** |
| Figures rendered | 94 |
| Figures showing `—` (no data) | **0** |
| Cards rendered | 71 |
| Chart canvases, all painted | 6 — Dashboard 1, Performance 3, Memory 2 |
| Horizontal document overflow | **0** on every screen |
| Console errors across the walk | **0** |

The only probe hit in the first run — `zero-size-svg: burger` on every screen —
is the mobile nav toggle, `display: none` at 1400 px and exactly **44 × 44** at
390 px. The probe was corrected to skip hidden subtrees.

### 8.4 · Load speed

Cold: a fresh browser context per route, signed in, then a direct hit on the
route's URL. Warm: one signed-in session, clicking through the sidebar.

| Route | FCP | DOMContentLoaded | data on screen | requests | transferred |
| --- | ---: | ---: | ---: | ---: | ---: |
| `/` | 40 ms | 32 ms | 63 ms | 31 | 28,745 B |
| `/lists` | 40 ms | 34 ms | 95 ms | 17 | 9,254 B |
| `/rules` | 48 ms | 41 ms | 102 ms | 14 | 7,359 B |
| `/policies` | 40 ms | 35 ms | 103 ms | 17 | 12,411 B |
| `/clients` | 44 ms | 37 ms | 104 ms | 14 | 8,956 B |

Warm SPA route switch, all thirteen:

| | route resolved | data on screen |
| --- | ---: | ---: |
| range | **11–25 ms** | **18–146 ms** |
| median | 14 ms | 63 ms |
| slowest | Live Feed 25 ms | Upstreams 146 ms |

Nothing here is near a budget. The figures are dev-box localhost against a
container on the same machine; they carry no RB5009 claim.

### 8.5 · Touch targets, measured as effective targets

The naive measurement — an element's own box — flags 15 controls under 44 px,
but the design deliberately grows the *target* rather than the drawn control
(`components.css:1418-1435`). Re-measured by probing a 44 × 44 square around
each control's centre with `elementFromPoint` and accepting a hit that lands on
the control, its label or its wrapper, scrolling the whole page so nothing below
the fold is missed:

| | |
| --- | --- |
| Controls measured at 390 px, all thirteen screens | **140** |
| Under an effective 44 px | **2** |

Both are recorded as findings **S9** and **S12**.

---

## 9 · Socket load

Two tabs, one signed-in session, ten minutes, against the ~3 qps DNS workload:
one parked on the **Live Feed** (subscribes `query`), one parked on the
**Dashboard** (subscribes `stats`).

| | Live Feed tab | Dashboard tab |
| --- | --- | --- |
| Frames sent by the client | `{"subscribe":["query"]}` | `{"subscribe":["stats"]}` |
| `query` frames received | **1,806** | **0** |
| `stats` frames received | 1 | 303 |
| Frames received before the client's `subscribe` landed | 1 (`stats`) | **0** |

**The non-subscribing page received no `query` frame in ten minutes**, across
~1,800 real DNS events — delivery filter and engine-side gate observed together.
The Dashboard's 303 `stats` frames over 600 s is the 2 s stats cadence, exactly.

The single `stats` frame on the Live Feed tab is the server's documented
open-by-default subscription (API.md §events: a socket receives everything until
its `subscribe` arrives). Measured here at **one frame**, and it was a `stats`
frame — no `query` frame reached a page that had not asked for one.

**The ring is bounded and the client does not grow:**

| minute | rows held | rendered | DOM nodes | JS heap |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 190 / 500 | 50 | 800 | 3.31 MB |
| 2 | 370 / 500 | 50 | 800 | 3.26 MB |
| 3 | **500 / 500** | 50 | 824 | 3.10 MB |
| 5 | 500 / 500 | 50 | 802 | 3.19 MB |
| 8 | 500 / 500 | 50 | 802 | 2.94 MB |
| 10 | 500 / 500 | 50 | 802 | 3.16 MB |

The ring saturates at its declared 500 in three minutes and **stays there** for
the remaining seven while 1,300 further events arrive. Rendered rows stay at the
page size, DOM nodes stay at ~802, and the heap oscillates in a 0.6 MB band with
no trend — this is a saturated-vs-saturated comparison, the one
[measurement-traps.md](../../measurement-traps.md) §Memory asks for.

**The engine shed nothing:** `counters.events_dropped` **0 → 0** across the run,
and `counters.swr.dropped` **0 → 0**.

---

## 10 · Mobile pass

Chromium at **390 × 844**, `isMobile`, `hasTouch`, dpr 3, iOS Safari UA, signed
in over TLS. All thirteen screens.

| Criterion (from the phone artboards) | Result |
| --- | --- |
| No horizontal body scroll at 390 px | **13/13 pass** — `scrollWidth − clientWidth` is 0 on `documentElement`, `body` and `.main`, and no element exceeds the viewport outside its own scroller |
| Wide content scrolls only inside its own container | pass — the only overflowing elements found are inside `overflow-x` scrollers |
| Every interactive control ≥ 44 px on its smallest axis | **138 / 140** — see §8.5 and findings **S9**, **S12** |
| Sidebar is an overlay drawer below 768 px | pass — parked at `x = −288`, opens to `x = 0`, width 288 of 390 |
| Dismissible by scrim **and** by close control | pass — both present; a scrim tap returns it to `x = −288` |
| Lists, Clients and Live Feed are one card per row, never a horizontal table | **pass** — 0 `<table>` elements rendered on any of the three at 390 px |
| Live Feed holds its narrow-viewport ring | pass — capacity reads **200**, not 500 |
| Live Feed stops rendering while the page is hidden | pass — 26 rows rendered before, **1** after 20 s hidden, while the ring kept filling to 86 |
| Mobile nav toggle target | 44 × 44 at 390 px, `display: none` at 1400 px |

### 10.1 · The real-device leg is not done and needs the owner

The task says *"every page opened on a real phone over the LAN"*. Everything
above is a device-emulated pass on the dev box. **A physical phone is the one
Stage-A item an agent cannot perform**, and the emulated pass does not
substitute for it on two points specifically: how the phone's browser treats the
self-signed certificate, and whether the session cookie survives on that
browser.

What is needed, when the owner has a moment:

1. The container publishes on the dev box's LAN address, port **18443**.
2. Open `https://<dev-box-LAN-IP>:18443/` on the phone and accept the warning.
3. Sign in, walk the thirteen screens, and confirm the drawer, the card-per-row
   layouts and the Live Feed against the artboards.

Note the certificate's SANs are `fastadhunter`, `localhost`, `127.0.0.1`, `::1`
and the **container's** address `172.17.0.3` — not the dev box's LAN address —
so a warning is expected here and says nothing about the shipped image.
**The certificate re-confirmation is a Stage B item** on the deployed build,
where the SAN set is the one `p5-02` fixed.

---

---

## 11 · Findings

No Stage-A check failed. Everything below was found while measuring and is
recorded because it is not visible from the code alone. **Nothing here has been
changed** — this task builds nothing, and the phase's review rule keeps fixes
behind an explicit approval.

### S1 · Minor · **FIXED** · The Live Feed's ring footnote rendered as broken English

`dashboard/frontend/src/pages/live-feed.tsx:159-162` builds the sentence from a
conditional and a fixed tail that repeat each other:

```text
The ring is 500 on a desktop one against 500 on desktop and 200 on a phone
— same bound, sized to the device, and fixed at the moment this page opened.
```

The conditional emits `a desktop one` (or `a narrow viewport`) and the tail then
restates both cases. **Rationale:** the two halves were written to cover the same
ground and one was never removed. **Impact:** a shipped, always-visible string on
the Live Feed read as a defect; it was the only ungrammatical copy found in the
build. **Verdict:** **fixed** — see §12.

### S2 · Minor · Three places state the runtime image has no timezone database, but it has one

| Location | Claim |
| --- | --- |
| `crates/fah-config/src/tz.rs`, module doc | "the image is distroless and ships no `/usr/share/zoneinfo`, so every clock in the process is UTC" and "no bundled IANA database (~1 MB of static data in an image that currently has none)" |
| [CONFIGURATION.md:219-221](../../../CONFIGURATION.md) | "POSIX TZ string, not an IANA name: the distroless image ships no timezone database" |
| `dashboard/frontend/src/pages/settings/metadata.ts:356` — **shipped UI copy** | same sentence, rendered on Settings under `[schedule] timezone` |

**Measured:** the runtime rootfs contains **1,308 `/usr/share/zoneinfo` entries**,
including `Europe/Bucharest` and the `UTC → Etc/UTC` symlink, on both
`linux/amd64` and `linux/arm64`. It arrives with
`gcr.io/distroless/static-debian12:nonroot` as a **4.24 MB** layer.

**Rationale:** the POSIX-TZ decision is sound on its other stated grounds — no
new dependency against the fixed tech stack, and RouterOS already speaks the
format. Only the premise is false, and the parenthetical size argument is
contradicted by the image it is arguing about. **Impact:** an operator reading
the Settings help is told something about the shipped artefact that is not true;
the `tz.rs` doc misstates why the code exists. No behavioural consequence — the
parser accepts POSIX TZ strings regardless of what is on disk. **Verdict:**
deferred, doc-only, and each of the three edits needs the owner's yes.

### S3 · Observation · 29 % of the shipped image is a timezone database nothing reads

4.24 MB of the 14.07 MiB arm64 rootfs. It cannot be dropped without leaving the
distroless base, which is out of scope here and is not worth doing at 49 % of
budget. Recorded so that a future image-size conversation starts from where the
bytes actually are: binary 10.9 MB, tzdata 4.24 MB, `/web` 0.61 MB, everything
else ~1.2 MB.

### S4 · Minor · The connection indicator ships four states; the phase file still says three

[plan/wip/phase5/CLAUDE.md](../../../plan/wip/phase5/CLAUDE.md) §"Two
consequences" says the indicator reads **live · not needed here ·
reconnecting**. The shipped app has a fourth, `api-unreachable` → **"API not
answering"** (added deliberately by `p5-07` F19 and documented at
`events/types.ts:36-46`), and `shell.tsx:88-96` substitutes `live` for
`not-needed-here` on any route whose own state is `not-needed-here` once a read
has answered.

**Measured:** across the thirteen screens, **twelve read `live`**. `Upstreams`
was captured reading `not needed here` only in the window before its first read
landed. So the state the phase file describes as "the correct steady state on
most of the UI" is in practice not what an operator sees; `live` on an
event-free screen means "the API answered", not "a socket is open".

**Impact:** documentation drift only — the behaviour is deliberate, coherent and
tested. **Verdict:** deferred; propose updating the phase file's two paragraphs
to describe four states and the substitution rule. Needs the owner's yes.

### S6 · Major · `PUT /api/v1/rules/user` accepts invalid lines whenever the document holds one valid rule

The Custom Rules page states, in shipped copy:

> Save is all-or-nothing. Every line is validated, then the whole set is swapped
> atomically. **One bad line means nothing is written — never a partial save.**

It is not true. Measured directly against the API:

| Document | Status | Stored |
| --- | --- | --- |
| `["this is not a rule at all"]` | **422** `line 1: invalid rule syntax` | nothing |
| `["junk one here","junk two here"]` | **422** `line 1: …; line 2: …` | nothing |
| `["!! a comment","junk two here"]` | **422** `line 2: …` | nothing |
| `["\|\|valid.example^","this is not a rule at all"]` | **200** | both lines, verbatim |
| `["this is not a rule at all","\|\|valid.example^"]` | **200** | both lines, verbatim |
| `["\|\|a.example^","!!x","bad line ###"]` | **200** | all three |

**The trigger is adblock format detection, not the number of good lines.** The
same junk line beside a hosts entry or a bare domain is refused; beside an
adblock rule it is accepted:

| Document | Detected format | Status |
| --- | --- | --- |
| `["0.0.0.0 ads.example.com","this is not a rule at all"]` | hosts | **422** `line 2` |
| `["ads.example.com","this is not a rule at all"]` | domain list | **422** `line 2` |
| `["\|\|a.example^","junk with spaces here"]` | adblock | **200** |

`routes.rs:1232` `validate_user_rules()` is correct — it rejects whenever
`fah_rules::parse_rule_list()` reports `parse_errors > 0`, and its own comment
explains that the block is parsed as one unit because "format detection needs
the whole text". The defect is upstream of it: **once the block is detected as
adblock, a line that is not a valid adblock rule stops being counted as a parse
error.** In the other two formats it is counted, and the endpoint behaves as
documented.

**This lands in `fah-rules`, which the orchestration plan reserves for Stage 2**
([resoak-orchestration.md](../../../plan/resoak-orchestration.md) §Stage 1:
*do not touch gen.py, fah-rules…*). It is therefore reported here and left
alone.

End-to-end confirmation of what that costs:

```text
PUT ["||only-valid.example^", "this is not a rule at all"]  → 200
telemetry.ruleset.rules                                      59 207 → 59 208
GET /api/v1/rules/user  → ["||only-valid.example^","this is not a rule at all"]
```

The junk line is stored, contributes **zero** compiled rules, and the operator
is told the save succeeded. The UI's invalid-line marker is driven by the API's
`422`, so on a `200` there is nothing to show: the page reads `4 lines` with no
warning.

[API.md:741](../../../API.md) states the contract this breaks: *"`PUT` validates
and atomically swaps; invalid lines → `422 validation_failed` with per-line
messages."*

**Impact:** the realistic case — pasting a block of adblock rules with a typo
somewhere in it — is exactly the case the validation does not cover. The typo is
stored, reported as saved, and never blocks anything. Both the page copy and
API.md already say what should happen. **Verdict:** a real defect, not a
dashboard one; owned by Stage 2.

### S7 · Minor · `POST /api/v1/lists` accepts a source that is neither a URL nor a path that exists

Through the Add-list dialog with the **List URL** kind selected and the source
set to `not-a-url`:

```text
201 POST /api/v1/lists
GET /api/v1/lists → { "id": "not-a-url", "url": "not-a-url",
                      "last_status": "failed",
                      "last_error": "read local list \"/data/not-a-url\":
                                     No such file or directory (os error 2)" }
```

The value was accepted, given an id derived from itself, and then treated as a
**mounted path** under `/data` despite the URL kind being chosen — so the kind
selector does not constrain what is sent, and nothing checks that a URL-kind
source carries a scheme. The dialog's own placeholder says
`the list's full URL, scheme included`.

**Impact:** low — the list simply never refreshes and says so on the Lists page.
But it is a persisted config entry created from a typo, and removing it is a
second deliberate action. **Verdict:** deferred; a scheme check on the URL kind
closes it.

### S8 · Minor · Two dialogs refuse to act and say nothing

| Dialog | Input | What happens |
| --- | --- | --- |
| Add a list | every field empty, **Add list** pressed | no request, dialog stays open, **no message** |
| Refresh interval | hours set to `0`, **Save** pressed | no request, dialog stays open, **no message** |

Both correctly refuse to send. Neither says why, and neither disables its submit
button, so the control looks live and does nothing when pressed. Every other
refusal in the app explains itself — the policy id field, the schedule window,
the rules editor. **Verdict:** deferred, cosmetic but a real dead-end.

### S9 · Minor · **FIXED** · The Memory page's Refresh button was a 15 × 15 target on a phone

`.hd-refresh` (`components.css:4298`) carries `border: 0; padding: 0` and no
enlarged target, so the button is exactly its icon: **15 × 15**, failing the
44 px criterion on both axes. Every other refresh control in the application is
a `.mini` inside a `.refresh-cluster` and passes.

Measured at 390 px across 140 controls on all thirteen screens; this was one of
only two that failed. **Verdict:** **fixed** — see §12.

### S10 · Minor · A cold unauthenticated load fires five rejected requests before redirecting

Opening `https://…/` with no session, the Dashboard route mounts and begins
fetching before the session guard redirects:

```text
401 GET /api/v1/lists
401 GET /api/v1/clients
401 GET /api/v1/cache
401 GET /api/v1/telemetry
WebSocket wss://…/api/v1/events → HTTP Authentication failed
```

then the login page renders. After signing in, the console is clean — **0**
errors across a full thirteen-screen walk.

**Impact:** five rejected auth attempts and one rejected socket upgrade per
unauthenticated hit, and five red errors in the console before the operator has
done anything. A LAN scanner touching `/` repeatedly multiplies it.
**Verdict:** deferred; the route's reads should wait behind the session check.

### S11 · Minor · A policy assignment accepts a malformed address

`POST /api/v1/policies` with `assignments: [{ "client": "999.999.999.999" }]`
answers **201**. The field's own hint reads *an address, a prefix such as
192.168.20.0/24, or a client name*.

A client **name** is free text, so an arbitrary string cannot be rejected on
sight — but a dotted quad whose octets exceed 255 is unambiguously a mistyped
address, never a name anyone means, and it will silently never match.
**Verdict:** deferred; reject only the dotted-quad-shaped values that are not
valid addresses.

### S12 · Nitpick · **FIXED** · One inline link was 16 px tall on a phone

`Open Memory →` on the Health page measured 94 × 16 with no enlarged target, so
its effective height was under 44 px. Its two neighbours — `Open Upstreams →`
and `Open Lists →` — passed only because of where the line happened to wrap,
which is not a property worth relying on. **Verdict:** **fixed** — see §12; the
fix applies to `.linky` as a class, so all four uses gain the target.

### S5 · Observation · The dev-box container's RSS is not comparable to the 128 MB row

| Figure | This container (amd64, Docker Desktop) | RB5009 (PERFORMANCE.md) |
| --- | ---: | ---: |
| `process_rss` | 115.9 MiB | 46.6–53.6 MiB |
| `process_peak_rss` | 163.2 MiB | — |
| `allocator_committed_bytes` | 222.6 MiB | — |
| `residual_bytes` | 114.1 MiB | — |
| `ruleset_bytes` | 1.8 MiB | 25.8 MiB at 799 k rules |

Different architecture, different host kernel, a 59 k-rule corpus rather than a
household one, and a synthetic 3 qps load. **The ≤ 128 MB budget row is an
RB5009 steady-state row and its three readings are a Stage B item.** Recorded
only so the dev figure is not later mistaken for a breach —
[measurement-traps.md](../../measurement-traps.md) §Memory applies in full,
including that `allocator_committed_bytes` is a lifetime high-water mark that
promises nothing.

---

## 12 · Fixes applied

Three findings were fixed on `phase5-10` at the owner's instruction. **S9** and
**S12** were the two that failed a written acceptance criterion — the phone
artboards' *every interactive control at least 44 px on its smallest axis*.
**S1** was a one-line copy fix taken in the same change.

| Finding | File | Change |
| --- | --- | --- |
| S9 | `styles/components.css` | `.hd-refresh` gains `position: relative` and a centred **44 × 44** `::before` |
| S12 | `styles/components.css` | `.linky` gains `position: relative` and a full-width, **44 px-tall** `::before` |
| S1 | `pages/live-feed.tsx` | the duplicated half of the ring sentence removed |

**Two decisions worth recording:**

1. **The target grows, the control does not.** Both CSS fixes copy the technique
   `.switch` already uses (`components.css:1404-1435`) rather than padding the
   drawn element: the glyph stays 15 px and the link stays inline at 12.5 px, so
   no layout moves and no artboard figure changes.
2. **`.linky` grows vertically only.** These are links at the end of a sentence;
   a horizontal overhang would reach into the prose either side for no gain, and
   all four uses already exceed 44 px wide. `.hd-refresh` grows both ways — its
   only neighbours in the header's actions slot are the residual badge and the
   timestamp, and neither is interactive, so nothing loses a tap.

The reworded sentence, read off the built image at both widths:

```text
1400 px  The ring holds 500 rows here — the bound is sized to the device,
         500 on a desktop viewport and 200 on a phone, and fixed at the
         moment this page opened.
 390 px  The ring holds 200 rows here — the bound is sized to the device,
         500 on a desktop viewport and 200 on a phone, and fixed at the
         moment this page opened.
```

### 12.1 · Re-verification

The images were rebuilt on **both** architectures and a container started from
the amd64 one, so what is re-measured below is the artefact, not a dev server.

| Check | Before | After |
| --- | --- | --- |
| Controls under an effective 44 px, 390 px, all 13 screens | **2** of 140 | **0** of 139 |
| Live Feed ring sentence | ungrammatical | correct at 1400 px and 390 px |
| Render faults across 13 screens | 0 | **0** |
| Figures rendering `—` | 0 | **0** |
| Horizontal document overflow | 0 | **0** |
| Console errors across a 13-screen walk | 0 | **0** |
| `tsc --noEmit` | clean | clean |
| vitest | 928 pass | **928 pass** |
| Bundle, gzip | 128,767 B | **128,730 B** (83.8 % of budget) |
| Bundle, brotli | 114,138 B | **114,097 B** |
| `/web` in the image | 614,035 B / 137 files | 614,181 B / 137 files |
| arm64 rootfs | 14,753,792 B | **14,753,792 B** (unchanged) |
| Node / toolchain paths in the runtime image | 0 | **0** |

The control count moves 140 → 139 because the re-verification container is
freshly started and only one client had appeared on the Clients page at the time
of the sweep. Every screen reports zero controls under target.

**S9, S12 and S1 are closed.** The remaining findings are unchanged: **S6** is
Major and owned by Stage 2 (`fah-rules`); **S2** and **S4** are documentation
edits awaiting the owner's yes; **S7**, **S8**, **S10** and **S11** are deferred
minors; **S3** and **S5** are observations.

---

## 13 · Stage A status

Every Stage-A item in the task file's §Execution split is complete and measured.

| Stage A item | Status |
| --- | --- |
| End-to-end tests against a running `fah-api` | **PASS** — 26/26, with a negative control proving 12 of 13 page assertions fail without data (§5) |
| Route-ordering regression re-asserted | **PASS** — 3 tests green plus live confirmation (§5.3) |
| Bundle: gzip, brotli, chunks, uPlot separately, login path | **PASS** — 128,730 B gzip, 83.8 % of budget (§3) |
| Image: size, phase delta, no Node / no toolchain / no source maps | **PASS** — arm64 14.07 MiB against 30 MB (§4) |
| Page-by-page figure trace | **PASS** — every figure traced, 33/33 verified live (§6) |
| Route-scoped fetching table, measured | **PASS** — all thirteen screens (§7) |
| Socket-load run | **PASS** — ring bounded, 0 `query` frames to a non-subscriber, nothing shed (§9) |
| Mobile pass over the LAN | **PARTIAL** — emulated pass complete and green (§10); the **real-phone leg needs the owner** (§10.1) |
| Gates, cargo and frontend, `request_coverage.rs` included | **PASS** (§2) |

Beyond the scope, at the owner's request: an exhaustive UI audit — every
control, every dialog, every save, every link, render correctness and load speed
on all thirteen screens (§8).

**Stage B is untouched and stays open.** The three RSS readings, Argon2id cost
on the device, polled-endpoint costs, the `constants.ts` refresh-default
correction and the certificate re-confirmation all need the RB5009 and all wait
for the p2.6-11 day-7 acceptance. The task sits **`AWAITING SOAK`**.

### Verdict

**PASS WITH DEFERRED FINDINGS.**

Nothing in Stage A failed. Three findings were fixed and re-verified (§12); the
two that mattered — **S9** and **S12** — were the only breaches of a written
acceptance criterion, and both are closed. **S6** is a genuine Major, predates
phase 5, lives in `fah-rules`, and is assigned to Stage 2 by the orchestration
plan. The rest are deferred minors, two documentation edits awaiting the owner's
yes, and two observations.

One row is not green and cannot be made green here: the real-phone leg of the
mobile pass (§10.1).
