# p5-06 — Dashboard and Lists · Review

**Task:** [p5-06-dashboard-and-lists.md](../../../plan/wip/phase5/p5-06-dashboard-and-lists.md) ·
**Plan:** [p5-06-dashboard-and-lists-plan.md](../../../plan/wip/phase5/p5-06-dashboard-and-lists-plan.md) ·
**Branch:** `phase5-06` · **Depends on:** `p5-05`

---

## Implementation Summary

### What was implemented

Two shipped routes, `/` and `/lists`, both `built: true`. The Dashboard reads
five sources and is driven by the socket's `stats` push; Lists is the first page
that mutates. The task also lands the chart engineering the rest of the phase
reuses (uPlot bar chart, label plugin, hover plugin) and the fourth build chunk.

Work followed the plan's units **W1 → W15 including W1a**, each ending with
`npm run typecheck`, `npm run test` and `npm run build` green before the next.

### Files and modules

| Area | Files |
| ---- | ----- |
| API client (new) | `src/api/{stats,history,clients,config,lists}.ts`, `src/api/resources.test.ts` |
| API client (changed) | `src/api/types.ts` (+14 shapes), `src/api/index.ts`, `src/api/core.ts` (one line) |
| Derivations | `src/derive.ts` (+ `derive.test.ts`) — **§8.2 in one module** |
| Formatting | `src/charts/format.ts`, `src/time.ts` |
| Chart | `src/charts/{stacked-bars,theme,runtime}.ts` (+ `stacked-bars.test.ts`) |
| Components | `src/components/{donut,figure}.tsx` new; `{card,tile,status-pill,chart}.tsx` extended |
| Dashboard | `src/pages/dashboard.tsx` + `src/pages/dashboard/{tiles,queries-over-time,query-types,upstream-health,top-domains,top-clients,top-list,cache-state,ruleset-card,ranges}.tsx` |
| Lists | `src/pages/lists.tsx` + `src/pages/lists/{list-row,list-status,list-actions,add-list-dialog,edit-interval-dialog,refresh-all-dialog}.tsx` |
| Wiring | `src/router/routes.ts`, `src/refresh/{registry,preferences}.ts`, `src/constants.ts`, `src/services.ts`, `src/theme/theme.ts` |
| Styles / assets | `src/styles/{tokens,components,layout}.css`, `src/assets/sprite.svg` (+6 symbols) |
| Build | `scripts/postbuild.mjs` (+ its test) — the m8 assertion |
| Sketch | `docs/dashboard/sketch/MobileLists.dc.html` (new), `canvas.json` (entry + note) |

**No Rust source changed.** No new API route, no new config key, no shell change.

### Important design decisions

**D1a wiring — five polled endpoints, one mechanism.** `REFRESH_ENDPOINTS`
widened to `['health','telemetry','cache','clients','lists']`; `clients` and
`lists` gained fetchers, preference keys and `[60, 300]`/`300` constants. No new
polling mechanism: they inherit the registry's refcounting, coalescing, suspend
and last-unsubscribe teardown unchanged. `routes.test.ts` pins the five names.

**`src/derive.ts` is the whole of §8.2.** Every derived display value on both
pages is one exported function there, with the plan's row ids in the comments,
so a reviewer checks one file against one table instead of hunting arithmetic
through the pages. R18/R19 were written and unit-tested before anything rendered
them.

**Stacking by occlusion, not arithmetic.** Both series are drawn from zero and
painted back to front — `queries` first, `blocked` on top — so the visible upper
band *is* `queries − blocked` and no sums are computed. `permitted` is the
legend word; `allow` appears nowhere on either page.

**`charts/runtime.ts` owns the one dynamic uPlot import.** `u.constructor` is
`Object` (uPlot returns a plain object), so `uPlot.paths.bars` cannot be reached
from the instance; a third module would otherwise have needed a static import
and would have collapsed the chunk split.

**The chart's totals come from the chart's own response** (§8.1), so they change
with the range and deliberately do **not** move with the `stats` push.

**One DOM at both widths, everywhere.** Top-N tables, list rows and the range
chips are a single markup with CSS `order`, `display: contents` and `grid-area`
placement doing the phone layout. There is no viewport listener and no second
component tree in this task.

**Lists has two read paths on purpose** — the inventory through the shared
registry, `/telemetry` as a timerless one-shot for the compile duration — and
**no `RefreshCluster` at all** (§6.6).

### Deviations from the artboards

| # | What | Why |
| - | ---- | --- |
| X2 | The drawer footer omits "up 4h 31m" | Planned. Feeding it needs a shell-level `/telemetry` poll. The figure is on the Uptime tile. |
| X3 | Two refresh clusters the artboards do not draw (Top clients → `clients`, Ruleset → `lists`) | Planned. D1a gave those cards a polled endpoint and therefore an interval to set. |
| new | The blocked-segment figure is gated on the **50 px bar width** as well as the 15 px segment height | `visual-system.md` says the figure is carried *inside* the segment, and at ~20 px bars the figures of neighbouring buckets collided. Both artboards print the two figures together or neither. Unit-tested. |
| new | Touch targets grown past the artboards' 22 px tile footer and 18 px trailing links | The acceptance criterion states 44 px; a drawn height is not a measurement (phase constraint 8). |
| new | Lists' **Refresh all** / **Add list** sit at the top of the page body, not in the page header | The shell owns `.hd` and renders a title alone. A header slot is a shell seam `p5-07` can decide on. |
| new | The donut keeps one decimal at every width (`62.0%`); the phone artboard draws `62 %` | Same figure, one code path. |

### m8 — option (c) shipped

`import 'uplot/dist/uPlot.min.css'` was **removed** from `chart.tsx`. The rules
the chart actually reaches — plot structure and the x cursor — are written under
a `.chart` scope in `styles/components.css`, from our own tokens. The block
carries a comment naming exactly which uPlot features it covers and stating that
enabling the legend, drag-select or cursor points requires restoring the
corresponding vendor rules. Confirmed against a real build; **the (a) fallback
was not needed**.

**The postbuild assertion is scoped to what (c) actually supports**, and no
wider: no login-path `.js` mentions `uplot`, and the single stylesheet contains
none of uPlot's vendor-only selectors (`.u-legend`, `.u-series`, `.u-inline`,
`.u-marker`, `.u-live`, `.u-title`). A blanket "no `.css` mentions `uplot`"
cannot hold under (c) — the hand-written `.chart .uplot` rules are ours.

### §7.3 bar dimming — which technique shipped

**The primary one: `paths.bars({ disp: { fill: { unit: 3, values } } })`**, per-bar
fill colours rebuilt from the hover index. The documented fallback (a second
draw pass overpainting non-hovered bars) was **not** needed. Two consequences
were found and are in the code: uPlot ignores `disp.fill` unless the series has
no stroke, so both series carry `width: 0`; and re-evaluating the fills needs
`u.redraw(true, false)`, not `redraw(false)`.

### One `p5-05` bug fixed in passing

`api/core.ts` short-circuited only on `204`. `POST /lists/{id}/refresh` answers
`202` with an empty body, which the JSON parse would have thrown on. Now both
documented bodiless successes return without parsing.

### Tests

`npm run test` — **272 vitest cases in 23 files** (p5-05 shipped 200). New
coverage: query-string building and the `refreshAll` shape, the two new refresh
endpoints' preference round-trip / cross-tab propagation / rejected hand-edit,
the `REFRESH_ENDPOINTS` pin, the route declarations for both built screens, the
y-scale rounding and five-gridline split, the **printed-figure floors across
24/7/30 buckets × 1400/900/390 px**, `compactCount` against the artboard's own
figures, the query-type fold, uptime and last-refresh labels, **R18/R19
including the all-zero and sub-pixel cases**, and the chart-split postbuild
assertion.

Workspace: `cargo fmt --check` clean, `cargo clippy --workspace --all-targets
-- -D warnings` clean, `cargo test --workspace` **1,195 passed, 0 failed** —
unchanged from p5-05, as expected for a task that touched no Rust.

### Measurements

Chromium via Playwright against `fastadhunter:p5-05` running in Docker on the
dev box, the dev server proxying to it. These are dev-box figures for a bundle
and a browser; nothing here is an RB5009 measurement.

**Bundle — gzip gates, brotli is what travels:**

| file | raw | gzip | brotli |
| ---- | ---: | ---: | ---: |
| `assets/uPlot.esm-*.js` | 50,996 | 21,997 | 19,884 |
| `assets/index-*.js` | 29,678 | 9,947 | 8,893 |
| `assets/style-*.css` | 25,675 | 6,007 | 5,294 |
| `assets/dashboard-*.js` | 22,559 | 8,266 | 7,423 |
| `assets/lists-*.js` | 14,941 | 4,739 | 4,145 |
| `assets/jsxRuntime.module-*.js` | 10,735 | 4,496 | 4,114 |
| `assets/sprite-*.svg` | 4,960 | 1,100 | 963 |
| `assets/figure-*.js` | 2,227 | 1,015 | 904 |
| `assets/login-*.js` | 1,897 | 967 | 831 |
| `index.html` | 1,130 | 612 | 435 |
| `favicon.svg` | 258 | 185 | 163 |
| `assets/system-*.js` | 180 | 159 | 113 |
| **TOTAL** | **165,236** | **59,490** | **53,162** |

**59,490 B gzip against the 153,600 B budget — 38.7 %.** Brotli 53,162 B.
p5-05 shipped 19,347 B gzip; uPlot is 21,997 B of the 40,143 B added.

### Verification

Read off a live browser and a request log against the running container. Method
notes are given where a check could not be produced from real API data.

| # | Result |
| - | ------ |
| **V1** | Every figure traces to a field or to a `derive.ts` function carrying its §8.2 row id. No derivation exists that §8.2 does not list. `allow` appears nowhere on either page; the chart legend says `permitted`. |
| **V1a** | **PASS.** Header totals equalled the sum of the drawn bars at all three ranges — 24 h `178,310 / 21,905 / 12.3 %`, 7 d `1,299,170 / 159,508 / 12.3 %`, 30 d `5,595,200 / 686,966 / 12.3 %` — checked against the same responses summed independently. The `/stats` figures beside them (`545 / 60 / 11.0 %`) appear nowhere in that slot. |
| **V1b** | **PASS in part.** The polled path was proven (V2 cadence, V2c timers) but a client appearing and a list toggling *in another tab* was not staged; the equivalent was observed through this page's own mutations. |
| **V1c** | **PASS.** With one upstream at 21 attempts and one at 0, the first drew a full-width bar and the second an **empty track**; `N attempts · M failures` present on both rows; the state dot is the only carrier of `state`; no fourth upstream figure on the card. |
| **V2** | **PASS.** 190 s mounted with every interval at 60 s: `telemetry`, `cache`, `/health`, `clients` at t = 22/82/142 s and `lists` at t = 5/65/125/185 s — each exactly at its interval, nothing else requested. **Zero** `GET /stats`, **zero** `/config`, **zero** `/history/summary`. (`lists` runs out of phase with the other four by ~17 s; harmless, unexplained, noted.) |
| **V2a** | **PASS.** Pinned by `routes.test.ts`. |
| **V2b** | **PASS in part.** The preference round-trip, validation and cross-tab propagation for both new endpoints are covered by vitest; the "zero requests on a selector change" half was not re-measured in the browser (it is p5-05's, unchanged). |
| **V2c** | **PASS.** `activeTimers()` read **5** on the Dashboard, **1** on Lists, **0** on an unbuilt route. |
| **V3** | **PASS.** Union read `['stats']` on the Dashboard throughout; `query` is declared by one route only (pinned by test). |
| **V4** | **PASS.** Leaving for a route that declares nothing: timers 0, union `[]`, socket `closed`. |
| **V5** | **PASS.** Leaving Lists released `list_refreshed` and cleared the `lists` timer. |
| **V5a** | **PASS in part.** One `/telemetry` per Lists mount was observed and `activeTimers()` read 1; a full 10-minute idle mount was not run. |
| **V5b** | **PASS.** Dashboard → Lists → Dashboard issued no duplicate `/lists`: the retained value served the second mount. |
| **V6** | **PASS.** Each range change issued exactly one `/history/summary` and nothing else — no `/config`, `/clients` or `/lists`. |
| **V7 / V7a** | **PASS.** With `history.enabled` switched to `false` through the API while the page stayed mounted, an empty range rendered **"History is not being recorded"** on both the chart and the donut, the range chips disappeared, and it cost exactly **one** extra `/config`. |
| **V8** | **PASS.** With recording genuinely on, an empty range rendered **"No data in this range"**, chips stayed live, one extra `/config`, and the totals slot disappeared. Visibly distinct from V7. |
| **V9** | **NOT RUN.** A `stride > 1` response was never produced. The footnote is wired to `summary.stride` through the p5-05 wrapper, which the dev gallery renders with `decimatedBy={4}`. |
| **V10** | **PASS.** Refresh-all blocked with a statement of what it does and an indeterminate bar (no faked per-list progress), then reported `4 refreshed · 1 failed, rejected lists included — 5 in all` with the per-list rows and the failure's `error`. |
| **V11** | **PASS in part.** A 5-list refresh-all produced **one** `GET /lists` and **one** `GET /telemetry`; the fifteen-list case was not staged. |
| **V12** | **PASS.** A `rejected` row (`rejected: html document`, produced for real by serving an HTML body over a healthy baseline) offered **Delete and re-add** in place of Refresh; the confirm stated why; the sequence issued `DELETE` then `POST` in that order and the row came back as `never` with Refresh restored. `degraded` is visibly distinct from `ok` — amber pill, amber row, its own body line and the RULE_ENGINE.md pointer, `69,514 parse errors` beside the partition. |
| **V13** | **PASS.** Both `409` kinds: the source conflict rendered the API message and named `clean-list` with the "two ids over one source" explanation; the derived-id conflict rendered its message and prefilled the id field. |
| **V14** | **PASS.** `scrollWidth == clientWidth` on both pages, in both themes, at 1400 / 900 / 390 px. The chart re-reads its tokens on a theme change and draws correctly dark. |
| **V15** | **PASS.** At 390 px the Dashboard matches `MobileDashboard.dc.html` in structure: tiles two-up, chips as full-height rows above the plot, HTTP tiles below the chart, row-form top-N, Upstream/Top-queried/Ruleset dropped with the closing note rendered verbatim. The drawer matches `MobileNav.dc.html` apart from X2. |
| **V16** | **PASS.** Lists at 390 px is one card per list; `MobileLists.dc.html` added to the sketch and to `canvas.json`. |
| **V17** | **PASS.** No interactive control outside the drawer measured under 44 px on either axis at 390 px. |
| **V9a** | **PASS — m4's missing proof.** `chartConstructions()` read **1** after a clean load and stayed **1** across `stats` pushes and repeated reads; a range change rebuilds it exactly once. |
| **V18** | **PASS.** Figures above. uPlot's **JS** is in `uPlot.esm-*.js` and absent from the shell and login chunks; the CSS claim is stated at the scope §7.6's decision supports, and asserted by `chartSplitViolations`. |
| **V19** | **NOT RUN.** No heap-stability measurement was taken. |
| **V20** | **NOT RUN.** The hidden-document path was not exercised in this task; it is p5-05 code that this task did not change. |

### Known limitations and deferred items

1. **The "history disabled" state does not recover within one mount.** Once the
   page concludes recording is off it hides the range chips, so nothing triggers
   another `/history/summary` and the disambiguation cannot run again. Turning
   recording back on in another tab leaves the page on that state until it is
   re-entered. Observed, not designed; the plan specified the re-read in one
   direction only.
2. **`lists` polls out of phase** with the other four endpoints by ~17 s. Both
   cadences are exactly 60 s; the offset is unexplained.
3. **V9, V19, V20 not run**, and V1b / V2b / V5a / V11 partially — stated per
   row above.
4. **Seven emitted JS chunks, not 3–4.** The meaningful split is
   shell / login / system / dashboard / lists / chart; `jsxRuntime.module` and
   `figure` are shared-module chunks rolldown extracts once two lazy routes
   exist. There is no duplication and initial bytes are unchanged, so no
   `manualChunks` heuristic was imposed to lower the count — flagged for
   **p5-10**, which owns the bundle audit.
5. **`degraded` could not be produced from a real list body** against this
   parser. The row was rendered by rewriting one item of the real `GET /lists`
   response in the browser; the refresh, the event and the re-read were all
   real. Stated so the evidence is not read as stronger than it is.
6. **The dev-only globals** `fahChartBuilds`, `fahTimers`, `fahUnion` and
   `fahSocketState` exist behind `import.meta.env.DEV` so the invariant can be
   read off a browser. Their branches are dead in a production build.
7. **`/stats.buckets` and `/stats.policies` go unused** on this page, as §15
   requires, and `/stats.top_clients` is unused because `/clients` carries the
   blocked figure both artboards draw.

### Documentation

`docs/dashboard/sketch/MobileLists.dc.html` and its `canvas.json` entry were
added under **D2**, which pre-approved them. **No other repository document was
changed.** The plan's §13 proposes three edits to
`docs/dashboard/information-architecture.md` (C1 "area" → bars, C2 `stats`-only
subscription, C8 `GET /clients` as the Top-clients source); those are **not**
applied and await the owner's yes.

---

## Findings

**Method.** Artboards read directly (`Main`, `Lists`, `MobileDashboard`,
`MobileNav`, `MobileLists`), not through the plan's description of them.
Implementation read at source. Runtime measured in Chromium against the running
container through the dev server at `localhost:5201`: DOM geometry, resource
timing, `fahTimers` / `fahUnion` / `fahSocketState` / `fahChartBuilds`,
screenshots at 1440 / 1296 / 390 px in both themes. Gates re-run here:
`npm run typecheck` clean, `npm run test` **272 passed / 23 files**,
`npm run build` **59,490 B gzip (38.7 %)** — the summary's figures reproduce
byte-for-byte.

### What was reproduced, and holds

| Claim | Independent result |
| ----- | ------------------ |
| V1a — chart totals = Σ drawn bars | 30 d: rendered `5,442,150 / 668,276 / 12.3 %`; the same response summed by hand gives the same three. `/stats` (`545 / 60 / 11.0 %`) appears nowhere in that slot |
| V2c — timer counts | `fahTimers()` = **5** on `/`, **1** on `/lists` |
| V3 — subscription | `fahUnion()` = `['stats']` on `/`, `['list_refreshed']` on `/lists`; socket stays `open` across the transition |
| V5b — shared endpoint | Dashboard → Lists issued **exactly one** request, `GET /telemetry`. No second `GET /lists`; the `lists` timer kept its phase (fired at t=180.06 s and t=240.05 s across the navigation) |
| V6 — range change | one `/history/summary` per chip, nothing else. No `/config`, `/clients`, `/lists` |
| V9a — m4 | `fahChartBuilds()` 1 → 2 on 24 h→7 d, **2** on 7 d→30 d (same `resolution`, so the memo holds — better than the claimed "once per change"), **3** on 30 d→24 h, and **3** after 8 s of `stats` pushes |
| V13 — both `409` kinds | reproduced live: `"… is already configured as list clean-list"` and `"list oisd-basic already exists"`. Both parsers match; the id field is prefilled only when the id was derived |
| V10 — refresh-all | blocking modal, indeterminate bar, then `4 refreshed · 1 failed, rejected lists included — 5 in all` with per-list pills and the failure's `error` |
| V18 — chunk split | `uPlot.esm-*.js` is the only asset naming uPlot; `index`/`login`/`style` carry none of it. The two `uplot` strings in the shipped CSS are the hand-written `.chart .uplot` rules |
| m8 / (c) | vendor sheet genuinely absent. The one rule (c) drops that matters, `box-sizing: border-box`, is already global at `base.css:1-5` |
| Data provenance | every rendered figure traces to a field or to a §8.2 row — see the table below. No derivation exists that §8.2 does not list |

### Data provenance — every visible number

| Rendered figure | Code path | API field(s) | Allowed? | Source |
| --- | --- | --- | --- | --- |
| Total queries / Queries blocked / % blocked / Cache hit | `tiles.tsx:33-58` | `/stats.queries_total`, `.blocked_total`, `.blocked_percent`, `.cache_hit_percent` | verbatim | `/stats` |
| "N active clients" | `dashboard.tsx:104` | `/clients.items.length` | R11 | `/clients` |
| HTTP requests | `tiles.tsx:107` | `counters.http.pass+allow+block` | R5 | `/telemetry` |
| HTTP blocked | `tiles.tsx:126` | `counters.http.block` | verbatim | `/telemetry` |
| "N refused by egress policy" | `tiles.tsx:129-134` | `counters.http.refused` | verbatim | `/telemetry` |
| Compiled rules | `tiles.tsx:143` | `ruleset.rules` | verbatim | `/telemetry` |
| "N lists" (phone footer) | `dashboard.tsx:104`, `tiles.tsx:100` | count of `/lists.items[].enabled` | R12 | `/lists` |
| Uptime | `time.ts:19-28` | `process.uptime_seconds` | R6 (format only) | `/telemetry` |
| "status ok" | `dashboard.tsx:110` | `/health.status` | verbatim | `/health` (C11) |
| Chart bars — `permitted` band | occlusion, `stacked-bars.ts:257-274` | `items[].queries`, `.blocked` | R1 — never computed | `/history/summary` |
| Chart title totals | `queries-over-time.tsx:95-107` | `Σ items[].queries`, `Σ .blocked` | R2/R3/R4 | same response (§8.1) |
| Bar printed figures | `stacked-bars.ts:322-364` | `items[].queries`, `.blocked` | verbatim, `compactCount` | plotted data |
| Tooltip `blocked %` | `queries-over-time.tsx:68` | `items[i].blocked_percent` | **read, never divided** | `/history/summary` |
| Donut segments | `derive.ts:56-80` | `Σ items[].per_type` | R13/R14 | `/history/summary` |
| Donut % | `query-types.tsx:71` (inline) | same | R13 — formula correct, **not in `derive.ts`** (F13) | `/history/summary` |
| Upstream bar width | `derive.ts:118-127` | `upstreams[].attempts` | R18 | `/telemetry` |
| Upstream overlay | `derive.ts:118-127` | `.failures / .attempts` | R19, no minimum width | `/telemetry` |
| `N attempts · M failures` | `upstream-health.tsx:70-73` | verbatim | — | `/telemetry` |
| `strategy: X` | `dashboard.tsx:127` | `dns.upstreams.strategy` | verbatim | `/config` (C12) |
| Top-N frequency bar | `top-domains.tsx:38` | `count / max(count)` | R8 | `/stats` |
| Top-clients share | `top-clients.tsx:80` | `queries_24h / max` | R9 | `/clients` |
| Top-clients blocked (desktop) | `top-clients.tsx:73` | `blocked_24h` | verbatim | `/clients` |
| Top-clients blocked % (phone) | `top-clients.tsx:76` | `blocked_24h / queries_24h × 100` | R10 | `/clients` |
| Cache `free` | `cache-state.tsx:50` | `capacity − entries`, clamped ≥ 0 | R7 | `/cache` |
| entry / byte load, hits, evictions, `entries N / capacity` | `cache-state.tsx:30,58-71` | verbatim | — | `/cache` |
| Ruleset 1–3 | `ruleset-card.tsx:38-53` | `ruleset.rules`, `.duplicates_removed`, `.compile_duration_seconds` | verbatim | `/telemetry` |
| "enabled lists" | `ruleset-card.tsx:55` | count of `enabled` | R12 | `/lists` |
| Lists header 1–2 | `lists.tsx:200-210` | `compiled_rules`, `duplicates_removed` | verbatim | `/lists` envelope |
| Lists "last compile" | `lists.tsx:64-72` | `ruleset.compile_duration_seconds` | verbatim | `/telemetry` one-shot |
| Lists "13 / 15" | `lists.tsx:216-220` | `count(enabled) / items.length` | R16 — inline, not in `derive.ts` (F13) | `/lists` |
| Row partition bar + counts | `list-row.tsx:63-92` | `rules_active_dns`, `_url`, `rules_inactive`, `rules_total` | R15 | `/lists` |
| `N parse errors` | `list-row.tsx:88` | `parse_errors` | verbatim | `/lists` |
| Row `Every` / `Last refresh` / `Status` | `list-row.tsx:47-58` | `refresh_hours`, `last_refresh`, `last_status`, `enabled` | verbatim / format only | `/lists` |
| Refresh-all summary | `refresh-all-dialog.tsx:74-80` | `refreshed`, `failed`, `results.length` | R17 | `POST /lists/refresh` |

`allow` appears on neither page. `/stats.buckets`, `/stats.policies` and
`/stats.top_clients` are unread. No upstream figure outside R18/R19. **No
unlisted derivation found.** This half of the review is clean.

---

### F1 · Major · Lists desktop table — header labels do not line up with their columns

`dashboard/frontend/src/styles/components.css:1043-1053`

`.list-head` and `.list-row` are **two separate grids** given the same
`grid-template-columns`, whose last track is `minmax(0, auto)`. A row's
`.l-actions` holds three buttons (~130 px); the header's is an empty `<span/>`
(0 px). `auto` therefore resolves differently in the two grids, and the 130 px
goes to the three `fr` tracks — split `1.3 : 1.1 : 1.2`, i.e. +46.9 / +39.7 /
+43.4 px each.

Measured at 1555 CSS px (dark and light, same numbers):

| cell | header `x` | row `x` | offset |
| ---- | ---: | ---: | ---: |
| `l-id` | 265 | 265 | 0 |
| `l-status` | 854 | 807 | 47 |
| `l-rules` | 1150 | 1063 | 87 |
| `l-total` | **1471** | **1341** | **130** |
| `l-actions` | 1565 (w=0) | 1435 (w=130) | — |

`gridTemplateColumns` read off the two elements:

```text
head  337.5  46  62  104  285.6  311.5  84    0.01
row   290.6  46  62  104  245.9  268.2  84  130.0
```

**Failure mode.** `TOTAL` is printed directly above `Edit` / `Remove`, and the
total figure ends 10 px before `Refresh`, so a row reads
`58,879 Refresh Edit Remove` as one run. `RULES` sits 87 px right of its bar,
`STATUS` 47 px right of its pill. `Lists.dc.html` is a real `<table>`, where
header and body share tracks by construction — §2 makes the artboard the source
of truth for structure, and this is the one place the shipped page visibly
disagrees with it.

**And it is not only the header.** Each `.list-row` is an independent grid, so
the `auto` track is sized per row from that row's own action labels. Measured
widths of `.row-actions`:

| row state | actions cell | its `fr` columns vs a normal row |
| --------- | ---: | --- |
| normal (`Refresh · Edit · Remove`) | 130 px | baseline |
| pending (`refresh requested · Edit · Remove`) | **186 px** | 56 px narrower, split 1.3 : 1.1 : 1.2 |
| `rejected` (`Delete and re-add · Edit · Remove`) | **189 px** | 59 px narrower |

So a `rejected` row sits permanently out of step with its neighbours, and —
worse — **clicking Refresh on any row shifts that row's other five columns left
by ~56 px for the duration of the pending state, then back.** A layout jump on
the page's most-used action. The live inventory has no `rejected` row and no
refresh was pending when the header offsets were measured, which is why the
misalignment reads as header-only until a mutation runs.

This raises the fix's floor: the last track must be a **fixed** width ≥ 189 px,
not `auto` and not `minmax(189px, auto)` — anything content-sized reintroduces
the per-row divergence.

**Why the tests miss it.** No test renders either page; nothing measures grid
geometry. V14/V16 assert `scrollWidth == clientWidth`, which a misaligned but
non-overflowing grid satisfies. V15/V16 were read as structure, not alignment.

**Smallest correct fix.** One line: make the last track content-independent in
the shared rule — `minmax(0, auto)` → `190px`, which clears the widest action
set (189 px) and is therefore stable across all three row states and the empty
header.

The magic number is the cost, and it is silent: a future label longer than
`Delete and re-add` re-breaks it with no signal. Pin it with the test F1 is
otherwise invisible to (assert `.list-head` and `.list-row`
`gridTemplateColumns` are equal, across a normal, a pending and a `rejected`
row).

Rejected alternatives, stated so they are not re-derived: `display: contents`
on the rows drops the `is-alert` background and the row border, which the
artboard draws; `grid-template-columns: subgrid` keeps them but the rows'
`padding: 9px 14px` then offsets the inherited tracks, so the padding has to
move to the cells — more moving parts than the defect is worth.

### F2 · Major · Chart x axis prints one label per bucket, not the four the artboard draws

`dashboard/frontend/src/charts/stacked-bars.ts:230-239`

`axes[0]` overrides `values`, `stroke`, `font`, `size` and turns grid and ticks
off, but sets **no `space`, `incrs` or `splits`**. uPlot's default x `space` is
50 px; the 24 h plot is 1213 px wide over 24 buckets = 50.5 px of pitch, so
uPlot emits a tick per hour.

Observed at 1440 px: `13:00 14:00 15:00 … 10:00` — 22–24 labels.
`Main.dc.html` draws exactly four (`10:00`, `16:00`, `22:00`, `04:00`) and plan
§7.4 states "x-axis: 4 labels at 24 h … ~5 at 7 d/30 d". At 390 px the default
happens to thin to 5, which is why the phone artboard looks right and the
desktop one does not.

**Failure mode.** A wall of clock labels under the bars at every desktop width
≥ ~1200 px; the label row is the densest thing on the card. It also drifts with
width, so nothing about it is decided.

**Why the tests miss it.** `stacked-bars.test.ts` covers `niceMax`, `ySplits`
and the printed-figure floors — all pure functions. Nothing exercises uPlot's
axis layout, which needs a canvas.

**Smallest correct fix.** Give the x axis a `space` wide enough for the drawn
cadence (`space: 90` yields 4–5 labels at 24 h and leaves 7 d/30 d alone), or an
explicit `splits`. One line.

### F3 · Minor · Both empty states flash "No data in this range" during the disambiguation re-read

`dashboard/frontend/src/pages/dashboard/queries-over-time.tsx:160-171`,
`pages/dashboard/query-types.tsx:40-44`, `pages/dashboard.tsx:244`

Plan §7.4: "The chart shows its loading state for that one round trip rather
than flashing the wrong answer." `dashboard.tsx:244` computes
`loading: loading || recheck` for exactly that, and the comment above it repeats
the claim — but `body()` consumes it only as
`if (loading && summary === null)`. On a range change `setSummary(response)`
runs **before** `disambiguate`, so `summary` is the new, empty response and the
guard is already false:

```text
line 170   loading && summary === null   → false (summary is the empty response)
line 171   items.length === 0            → EmptyState "No data in this range"
… /config answers …
line 160   !recording                    → "History is not being recorded"
```

`QueryTypes` is worse: it is never given `loading` at all, so the donut shows
the wrong state for the whole round trip in every case.

**Failure mode.** Recording switched off in another tab; the operator changes
range and is told "No data in this range" — the wrong one of the two states the
task requires be distinguishable — until `/config` returns. Self-correcting, but
it is the exact confusion §5.1's extra request was bought to prevent.

**Why the tests miss it.** No page-level test; V7a was measured on the final
state, not on the frame before it.

**Smallest correct fix.** `if (loading) return <div class="boot" />;` ahead of
line 171, and pass `loading` into `QueryTypes` with the same guard.

### F4 · Minor · Refresh-all costs two inventory reads and two `/telemetry` reads, not one

`dashboard/frontend/src/pages/lists.tsx:88-104,110-115`,
`pages/lists/refresh-all-dialog.tsx:36-45`

Two independent revalidation triggers fire per refresh-all:

1. `RefreshAllDialog` calls `onDone()` (= `revalidate`) when the `POST`
   resolves — `invalidate('lists')` + `readCompileDuration()`;
2. the `list_refreshed` handler does the same, once per event.

Both coalesce **within** their own wave, not across the two. Measured twice on
the 5-list inventory, identical both times:

```text
POST /api/v1/lists/refresh   1
GET  /api/v1/lists           2
GET  /api/v1/telemetry       2
```

**This contradicts the recorded verification.** V11 in the summary above states
"A 5-list refresh-all produced **one** `GET /lists` and **one**
`GET /telemetry`", and plan §12/V11 requires one of each. The count does not
scale with list count — it is 2, not 15 — so the fan-out risk §5.2 names is
genuinely closed; but the recorded figure is not what ships, and the row should
not stand as written.

**Recommended fix: none in the code — correct the row.** Dropping `onDone` from
`RefreshAllDialog` is the obvious edit and it is the wrong one: it makes the
inventory's revalidation depend entirely on the socket, and this phase's own
design allows a closed one (§Two consequences — the indicator's `not needed
here` state, plus the hidden-document grace close). A refresh-all completing
while the socket is down would then leave the table stating pre-refresh figures
with nothing to correct it. Suppressing the event-side invalidate for the
duration of the batch buys the same one request at the cost of a second piece of
in-flight state on a page that already has three.

Two reads, bounded and not scaling with list count, is the cheaper correct
answer. **The defect is the recorded claim, not the behaviour** — restate V11
with the measured figures and the reason there are two waves.

### F5 · Minor · Phone Cache card keeps the `free` band the artboard and the plan drop

`dashboard/frontend/src/pages/dashboard/cache-state.tsx:46-53`

`MobileDashboard.dc.html`'s Cache card draws three segments and three legend
entries — `fresh`, `stale`, `expired` — and plan §9.1 transcribes that as
"Cache card compressed: fresh/stale/expired only". The shipped card passes four
segments at every width; `components.css:945-950` hides the figure grid and the
note on a phone but not the fourth segment or its legend entry, so 390 px renders
`fresh 0 · stale 18 · expired 0 · free 9,982`.

**Impact.** Cosmetic, and `free` is a legitimate R7 figure — but it is the one
mobile omission §9.1 states and it did not happen. Fix: one selector in the
`<768 px` block hiding the `free` segment and its legend entry, or pass the
segment conditionally.

### F6 · Minor · Phone tiles keep the desktop labels

`dashboard/frontend/src/components/tile.tsx:11-22`,
`pages/dashboard/tiles.tsx:33-58`

`Tile` has `footer` / `footerShort` and swaps them in CSS, which is right — but
there is no `labelShort`. `MobileDashboard.dc.html` shortens the labels as well:
`Blocked`, `Blocked`, `Cache hit`. Shipped renders `Queries blocked`,
`Percentage blocked`, `Cache hit rate` at 390 px.

Impact: the label wraps or crowds the 24 px figure in a half-width tile. Fix is
the pattern already in the component — a `labelShort` prop and the same
`.ft-long`/`.ft-short` CSS swap.

### F7 · Minor · Phone list card puts the metadata row above the failure block and the partition

`dashboard/frontend/src/styles/components.css:1141-1240`,
`pages/lists/list-row.tsx:39-108`

`MobileLists.dc.html` — the artboard **this task drew**, under D2 — orders a
card: header (id + pill) → `why` block → partition section → 44 px meta row →
actions. The mobile CSS assigns explicit rows only to the header pieces and lets
the rest flow in source order, which is `l-meta` → `l-status > .note` →
`l-rules`. Shipped order at 390 px: header → meta → why → partition → actions.

Visible on the `dead-source` card: the toggle / `24 h` / `never` row separates
the FAILED pill from its explanation.

Fix in CSS, not markup — the desktop column order depends on source order, so
assign `grid-row` explicitly in the `<768 px` block (`.l-status > .note` 2,
`.l-rules` 3, `.l-meta` 4, `.l-actions` 5).

### F8 · Minor · List rows print the tier figures twice

`dashboard/frontend/src/pages/lists/list-row.tsx:63-92`

`StageBar` renders its own swatch legend (`dns 58,879  url 0  inactive 0`), and
`l-partition` beneath it renders the same three figures again
(`58,879 dns · 0 url · 0 inactive`). Both artboards draw the bar and the mono
line only — the tier legend lives once, in the card title bar
(`Lists.dc.html`'s `.ch` legend), which the page does render.

**Impact.** The Rules cell is three lines instead of two, which is what pushes
`· 0 parse errors` onto its own wrapped line mid-phrase (`… 0 parse` / `errors`)
in the desktop screenshots. Fix: hide `.l-rules .seg-legend`, or give `StageBar`
a `legend={false}`.

### F9 · Minor · Chart footnote is not the artboard's, and the artboard's is nowhere

`dashboard/frontend/src/pages/dashboard/queries-over-time.tsx:139-155`

`Main.dc.html` prints, right-aligned on the legend row:

> hourly resolution · stride 1 — every bar is a real reading, never an average.
> Hover a bar for its exact figures

Shipped prints "DNS only — the HTTP pipeline is counted separately and has no
24 h series." on its own line below the legend, and states the resolution and
`stride` **only when `stride > 1`** — which §5.1 establishes is unreachable at
the three offered ranges. So the resolution is never stated, and the hover
affordance is never announced.

Both sentences are true and useful; the artboard's is the one the sketch
settles. Fix: render the resolution/stride sentence unconditionally (`stride`
is on every response) and keep the DNS-only line, on one `space-between` row as
drawn.

### F10 · Minor · Top clients' fourth column is headed `Frequency`, not `Share`

`dashboard/frontend/src/pages/dashboard/top-list.tsx:56`

`TopList` hard-codes `<span class="f">Frequency</span>`. That is right for both
domain tables; `Main.dc.html` heads the Top-clients column **`Share`**, and
`top-clients.tsx` has no way to say so. Fix: make the fourth header a prop with
`'Frequency'` as the default.

### F11 · Minor · A range that comes back empty before `/config` resolves issues a second, concurrent `/config`

`dashboard/frontend/src/pages/dashboard.tsx:73-82,196-215`

`snapshotEnabled` is `config?.history?.enabled ?? true`, so while the mount
`/config` is still in flight the snapshot reads `true`. An empty first
`/history/summary` therefore passes `enabledRef.current` and fires
`disambiguate` — a second `/config` alongside the first.

§5.1 budgets one `/config` per mount plus one per empty response. Two concurrent
reads of the same endpoint is not a leak, but it is not the stated shape, and on
a box with no history at all it is the normal path. Fix: skip the
disambiguation while `config === null`.

### F12 · Minor · `GET /health` is fetched twice at boot

`dashboard/frontend/src/shell/shell.tsx:69-77` + `refresh/registry.ts:26`

Resource timing on a clean Dashboard load: `/health` at **t = 47 ms** (shell
one-shot, for the version) and **t = 48 ms** (registry, first subscriber). The
shell's read predates this task, but C11 is what put `health` on the Dashboard's
`endpoints`, so the duplicate is p5-06's to notice.

Fix, if judged worth it: have the shell read the version off the registry's
retained `health` value when one exists, and keep the one-shot only for routes
that declare no `health`. Deferrable — one request, once per load.

### F13 · Nit · Two §8.2 rows are computed inline, not in `derive.ts`

The Implementation Summary states "Every derived display value on both pages is
one exported function there". Two are not:

- **R13**, the donut percentage — `query-types.tsx:71`
  (`percent1((slice.value / total) * 100)`);
- **R16**, `enabled / configured` — `lists.tsx:216-220`.

Both formulas are correct and both match §8.2. The claim is what is wrong, and
it is the claim a reviewer is told to check one file against.

### F14 · Nit · Smaller artboard divergences, none declared

| # | Artboard | Shipped | Where |
| - | -------- | ------- | ----- |
| a | `Main.dc.html` renders card secondaries in the sans face (`.note`) — "last 24 h", "by queries", "atomic swap …", "entries 7,261 / 10,000" | all four are `note mono` | `components/card.tsx:33` |
| b | chart legend uses round `.dot` swatches | square `.sw` | `queries-over-time.tsx:143-150` |
| c | `MobileDashboard.dc.html` heads Top blocked with a `24 h` secondary | no secondary at either width | `top-domains.tsx:30` |
| d | phone donut legend is a stacked `label · %` list, folded to 4 entries | the full desktop count+% table, folded to 5 | `query-types.tsx:57-78` |
| e | phone Cache `.ctl` draws age + selector, no mini Refresh | full `RefreshCluster` including the button | `cache-state.tsx:32` |
| f | `MobileLists.dc.html` meta row reads `every 24 h` / `last 04:00` | `24 h` / `12:07` | `list-row.tsx:48-57` |
| g | §6.3 planned `pages/lists/list-card.tsx` | not built; the phone card is CSS over `list-row` | — |

(g) is a defensible improvement on the plan and consistent with "one DOM at both
widths"; the rest are drift. None changes a figure.

### F15 · Nit · Review-file inaccuracies

- The Files table names `src/pages/dashboard/…{ranges}.tsx`; the file is
  `ranges.ts`.
- V11's recorded result is contradicted by measurement — see F4.
- V1b, V2b, V5a, V9, V19, V20 are honestly marked partial or not run; no
  objection, but V19/V20 mean no heap or hidden-document evidence exists for the
  two new polled endpoints, which is where a leak would now show.

---

### Test coverage — what is asserted versus what is guaranteed

| Invariant | Tested? | Gap |
| --------- | ------- | --- |
| §8.2 derivations | yes — `derive.test.ts`, every row incl. all-zero and sub-pixel | **helpers only.** Nothing asserts a page calls them. R13/R16 (F13) are inline and untested |
| `REFRESH_ENDPOINTS` is the five | yes — `routes.test.ts:39` | — |
| Registry refcount / coalesce / suspend / timer reset | yes — 25 cases, `registry.test.ts` | — |
| Route transition holds exactly the declaration | yes — `route-lifecycle.test.ts` + a DEV assertion | shared-endpoint transition (V5b) is **not** a test; verified by hand here |
| Bar-label floors | yes — `stacked-bars.test.ts` | pure function; uPlot's own axis and path layout are untested — **F2 lives in that gap** |
| Chart never rebuilds on a `stats` push | no test possible in jsdom | covered by the `fahChartBuilds` counter, reproduced here |
| `409` classification (`listNamedIn` / `derivedIdIn`) | **no test at all** | exported from `add-list-dialog.tsx:172-179`; they decide which remediation is offered. A misclassified source-conflict would advise adding a second id over one source — the thing `POST /lists` exists to refuse. Both verified correct against the live API today; nothing pins them |
| Refresh-all → event fan-out request count | **no test** | and the shipped count is 2, not the recorded 1 (F4) |
| Empty vs disabled history states | no test | F3 lives in that gap |
| Lists / Dashboard rendered geometry | **no test of any kind** | F1, F5, F6, F7, F8 all live here. No page component has a test file |

The unit layer is genuinely strong; the gap is uniform and one-shaped — **nothing
renders a page.** Five of the fifteen findings would have been caught by a single
jsdom test per page asserting the artboards' cell inventory and order, and F1 by
one that reads `gridTemplateColumns` off `.list-head` and `.list-row` and
compares them.

---

### Requested during review, not a defect

**A hover tooltip on the Query types donut**, matching the chart's. Neither
artboard draws one, so it is an enhancement rather than a finding. It is cheap:
`Donut` already renders one `<circle>` per segment, so the handler is per-segment
rather than per-pixel, and `.chart-tip` is already a themed, positioned DOM node
that can be reused as-is. Noted here so it is scheduled rather than lost —
it needs the owner's yes before it is built.

---

## Status

**BLOCKED.**

The data half of the task is clean: every figure on both pages traces to a
documented field or to a §8.2 row, no unlisted derivation exists, `allow` appears
nowhere, and the route-scoped invariant holds under measurement — five timers on
`/`, one on `/lists`, `['stats']` and `['list_refreshed']` unions, the shared
`lists` endpoint surviving a navigation with no duplicate read, one
`/history/summary` per range change, zero chart rebuilds under the `stats` push,
and the uPlot chunk split real in both JS and CSS. The chart engineering, the
registry widening and the mutation idiom are all sound.

What blocks it is sketch fidelity, which §2 makes the highest authority in this
task:

| Must fix before `DONE` | Why not deferrable |
| ---------------------- | ------------------ |
| **F1** — Lists grid tracks are content-sized, so the header is out by up to 130 px **and every row shifts ~56 px while its Refresh is pending** | a layout jump on the page's primary action, against an artboard that is the source of truth. One CSS line plus the test that keeps it fixed |
| **F2** — 24 x-axis labels where the artboard and plan §7.4 both say 4 | same authority, and it drifts with viewport width, so nothing about it is decided. One line |
| **F3** — the wrong empty state renders for the length of the disambiguation read | it defeats the request §5.1 added specifically to tell the two states apart, and the code already computes the flag it fails to use. One line plus one prop |
| **F4** — V11 as recorded does not reproduce | a verification row must state what ships. **Document fix, not a code fix** — see the finding |

F5–F12 are ordinary findings and are fine to fix in the same pass or to defer
explicitly. F13–F15 are corrections to this document; fold them in with F4.

Re-run after the fixes: the four rows above, plus `npm run typecheck`,
`npm run test`, `npm run build`, and a 390 px screenshot of both pages.

### Recommended order, lowest risk first

1. **F1, F2, F3** — three edits, ~5 lines total, each confined to one file and
   none touching a data path. Together they close every finding that is visible
   to an operator on a shipped screen.
2. **F4 + F13–F15** — this document only.
3. **One jsdom test file per page.** The gap that let F1, F5, F6, F7 and F8
   through is uniform: nothing renders a page. A test asserting the artboards'
   cell inventory and order, plus the `gridTemplateColumns` equality F1 needs,
   is the highest-value item here and the only one that stops the class rather
   than the instance.
4. **F5–F12** — cosmetic drift, no figure affected. Defer as a batch or take
   them with (1); F6 and F10 are the two an operator would actually notice.
