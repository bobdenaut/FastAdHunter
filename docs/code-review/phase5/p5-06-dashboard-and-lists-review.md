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
| Dashboard | `src/pages/dashboard.tsx` + `src/pages/dashboard/{tiles,queries-over-time,query-types,upstream-health,top-domains,top-clients,top-list,cache-state,ruleset-card}.tsx` + `ranges.ts` |
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

> **Correction (fix pass, F13).** As shipped for review this was true of every
> row but two: R13's percentage was divided inline in `query-types.tsx` and R16
> is two verbatim counts printed either side of a slash. R13 is now
> `derive.sliceShare` and the sentence holds again; R16 stays where it is, and
> is a formatting of two fields rather than a derivation.

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
| **V11** | ~~**PASS in part.** A 5-list refresh-all produced **one** `GET /lists` and **one** `GET /telemetry`.~~ **WRONG AS RECORDED — corrected by F4.** Re-measured four times on the 5-list inventory, identical every time: **two** `GET /lists` and **two** `GET /telemetry`, from two coalescing groups that do not overlap (the dialog's completion callback, then the `list_refreshed` wave). Bounded and independent of list count, so the fan-out risk §5.2 names is closed; the figure is not one. The fifteen-list case was not staged. |
| **V12** | **PASS.** A `rejected` row (`rejected: html document`, produced for real by serving an HTML body over a healthy baseline) offered **Delete and re-add** in place of Refresh; the confirm stated why; the sequence issued `DELETE` then `POST` in that order and the row came back as `never` with Refresh restored. `degraded` is visibly distinct from `ok` — amber pill, amber row, its own body line and the RULE_ENGINE.md pointer, `69,514 parse errors` beside the partition. |
| **V13** | **PASS.** Both `409` kinds: the source conflict rendered the API message and named `clean-list` with the "two ids over one source" explanation; the derived-id conflict rendered its message and prefilled the id field. |
| **V14** | ~~**PASS.**~~ **WRONG AS RECORDED — corrected by F16.** `/lists` at 900 px measured `scrollWidth 964` against `clientWidth 883`: **81 px of body scroll**, not zero. `/` was clean at all three widths, and both pages were clean at 1400 and 390 px. Fixed and re-measured — see the fix pass. The chart does re-read its tokens on a theme change and draws correctly dark. |
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

### F16 · Major · `/lists` scrolls the page body sideways at 900 px

`dashboard/frontend/src/styles/components.css:1039-1053` (as reviewed)

Found while measuring F1's fix, and it is the same root cause seen from the
other side. The eight columns' minimums add up to
`150 + 46 + 62 + 104 + 160 + 190 + 84` plus seven 10 px gaps and 28 px of row
padding — more than the content area between 768 and 1199 px. `.bd.lists-body`
had `overflow-x: visible`, so the row overflowed the card, the card overflowed
the page, and the body scrolled.

Measured at a 900 px viewport, dark and light alike:

| page | `scrollWidth` | `clientWidth` |
| ---- | ---: | ---: |
| `/` | 883 | 883 |
| `/lists` | **964** | **883** |

The actions track was resolving to **0 px** at that width, so the buttons were
being squeezed out of their own column as well.

**This contradicts two recorded statements.** V14 records `scrollWidth ==
clientWidth` "on both pages, in both themes, at 1400 / 900 / 390 px", and plan
§9.3 states "the page body never scrolls horizontally at any width".
visual-system.md §Responsive already says what should happen instead at this
band: "tables scroll inside their own container".

**Why the tests miss it.** The same gap as F1 — no layout is computed anywhere
in the suite, and the browser pass that produced V14 evidently checked 1400 and
390 px and carried 900 forward.

**Smallest correct fix.** `overflow-x: auto` on `.bd.lists-body`, plus
`min-width: min-content` on `.lists-table` (desktop only) so the columns keep
their widths inside the scroller rather than collapsing.

### F17 · Minor · The chart tooltip is barely visible in the dark theme

`dashboard/frontend/src/styles/components.css:519-533` (as reviewed)

Reported by the owner against a running dark-theme page. `.chart-tip` was
`rgba(31, 39, 51, 0.95)` with no border — the artboard's dark slab, which
separates from a **white** card by value alone. In the dark theme the card
behind it is `--surface: #18212c`, so the slab sits about seven units away from
its own background and reads as a faint rectangle.

Two further token-wall breaches in the same block: `color: #fff` and
`.chart-tip-row .blocked { color: #e08a86 }` are literal colours, which
`tokens.css`' own header forbids ("no rule outside this file may name a literal
colour").

**Smallest correct fix.** Five tokens — surface, border, text, label, blocked —
defined in all three palette blocks, with the dark set raising the surface above
the card instead of sinking into it, plus a border and a shadow so the overlay
reads in both themes rather than relying on value in one.

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

### Requested during review, not a defect — **built, and it changed shape**

**A hover on the Query types donut.** Neither artboard draws one, so this is an
enhancement rather than a finding. Asked for as "a hovertip exactly like the
Queries over time card", built that way first, and then **deliberately not
shipped that way** — the reason is worth keeping:

| Step | What happened |
| ---- | ------------- |
| 1 | `.chart-tip` reused verbatim on the ring — same classes, tokens and three-line shape |
| 2 | It **covered the legend**, which prints the same count and share it was repeating. A bar carries no figures of its own, so the chart *has* to raise a tooltip; a ring drawn beside its own legend does not |
| 3 | Owner's call: mark the row instead. Pointing at a slice marks its legend row and dims the other arcs; pointing at a row lights its arc. The fact stays in one place and is made findable rather than duplicated into an overlay |
| 4 | First mark used `--surface-sunken` — **four units from the card in dark**, invisible. Same mistake as F17, one card over |
| 5 | Shipped: the mark is a **tint of the slice's own colour** (`color-mix(… 14%, var(--surface))`) plus a 3 px accent bar in that colour, so it says *which* arc, and a neutral tint cannot be dark enough for one theme and pale enough for the other |

One bug found and fixed inside step 5, worth recording because it is silent:
**`color-mix(in srgb, X 18%, transparent)` computes to ~1.8 % alpha, not 18 %** —
mixing with a fully transparent colour premultiplies. Measured off the computed
style (`oklab(… / 0.0181)`) rather than judged by eye. The fix is to mix over an
opaque colour: `color-mix(in srgb, var(--slice) 14%, var(--surface))`.

Verified in both themes at desktop, with five cases in `cards.test.tsx`: no
tooltip is raised, a slice marks its row, a row lights its slice, leaving clears
both, and each row carries its slice's token as `--slice`.

**Files:** `components/donut.tsx` (`hovered` / `onHover`, per-segment dimming),
`pages/dashboard/query-types.tsx` (the shared `hovered` state and the legend's
half of it), `styles/components.css`.

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

---

## Fix pass — applied and re-measured

Approved by the owner after the review above. Nine findings fixed, six deferred.
**No plan, task file or other repository document was changed**; the only
document edited is this one, and only to correct its own errors (F13, F15) and
record what follows.

### Fixed

| # | Change | Files |
| - | ------ | ----- |
| **F1** | last grid track `minmax(0, auto)` → `200px`, so the header and every row resolve the same tracks whatever a row's actions say | `styles/components.css` |
| **F2** | x axis gains `space: (…, dim) => dim / 5` — a label per fifth of the plot instead of uPlot's flat 50 px | `charts/stacked-bars.ts` |
| **F3** | the empty branch now waits on `loading`, and `QueryTypes` is given the same flag | `pages/dashboard/queries-over-time.tsx`, `query-types.tsx`, `dashboard.tsx` |
| **F6** | `Tile` gains `labelShort`; the three phone labels the artboard shortens are supplied and swapped in CSS beside the existing footer swap | `components/tile.tsx`, `pages/dashboard/tiles.tsx`, `styles/components.css` |
| **F8** | `.l-rules .seg-legend` hidden — the tier figures were printed twice per row | `styles/components.css` |
| **F10** | `TopList` gains `frequencyLabel`, defaulting to `Frequency`; Top clients passes `Share` | `pages/dashboard/top-list.tsx`, `top-clients.tsx` |
| **F13** | R13 moved out of the card into `derive.sliceShare`, so §8.2 is again checkable against one module | `derive.ts`, `pages/dashboard/query-types.tsx` |
| **F16** | `overflow-x: auto` on `.bd.lists-body` and `min-width: min-content` on `.lists-table` (≥768 px only) | `styles/components.css` |
| **F17** | five tooltip tokens in all three palette blocks, the dark set raised above the card, plus a border and a shadow; two literal colours removed from `components.css` | `styles/tokens.css`, `components.css` |

F4, F14 and F15 were **document** fixes, applied above: V11 and V14 are struck
through and restated with the measured figures, the `ranges.ts` filename is
corrected, and the `derive.ts` claim carries its correction inline.

**F4 deliberately changed no code.** Dropping the dialog's completion callback
would make revalidation depend entirely on the socket, which this phase's own
design allows to be closed; a refresh-all finishing while it is down would then
leave the table stating pre-refresh figures with nothing to correct it. Two
bounded reads is the cheaper correct answer.

### Deferred, with reasons

| # | Why it waits |
| - | ------------ |
| **F5** | phone Cache card keeps the `free` band. One selector, but it touches the segment set a figure is read from, and `free` is a legitimate R7 reading — worth doing beside `p5-08`'s Cache page rather than alone |
| **F7** | phone list-card section order. The fix is `grid-row` assignments in the `<768 px` block, which re-flows five cells; higher regression risk than the rest of this pass for a card that is already legible |
| **F9** | the artboard's chart footnote. Wording, and it interacts with the decimation footnote `Chart` owns — a wording change wants the owner's eye, not a reviewer's |
| **F11** | second concurrent `/config` when an empty range beats the mount read. One request, on a box with no history at all |
| **F12** | `/health` fetched twice at boot. Inherited shell shape; the fix belongs with whoever revisits `shell.tsx` |
| **F14** | six small artboard divergences: mono card secondaries, round vs square legend swatches, the Top-blocked `24 h` secondary, the phone donut legend, the phone Cache refresh button, `every`/`last` wording |

### Gates, re-run in full

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | **328 passed / 26 files** (was 272 / 23 — three new files, 56 new cases) |
| `npm run build` | **59,869 B gzip against 153,600 B — 39.0 %**, brotli 53,516 B. Was 59,490 B / 38.7 %; +379 B, of which ~190 B is the donut highlight |
| postbuild assertions | pass — the build exits non-zero on a violation and did not |
| Rust workspace | **not re-run.** No Rust source was touched in this pass or in the task; last green was `1,195 passed, 0 failed` |

### New tests — what they pin

| File | Cases | Pins |
| ---- | ----: | ---- |
| `styles/grid-tracks.test.ts` | 7 | F1 and F16 **from the stylesheet**, because jsdom computes no layout: the Lists template is eight tracks, its last is a fixed length, that length clears the widest action set (≥ 189 px), no other track is content-sized, the container scrolls, the grid is floored at `min-content`, and the row's duplicate tier legend stays hidden |
| `pages/lists/list-row.test.tsx` | 21 | cell inventory and order against `Lists.dc.html`, the five `last_status` values plus `disabled`, the secondary line's presence and absence, the alert tint on a live failure but not a disabled one, `never`, the disabled interval dash, and the three action sets |
| `pages/dashboard/cards.test.tsx` | 26 | both tile rows' labels, short labels, footers, short footers and figures; R5's sum excluding `refused`; the "since restart" caption; `Frequency` vs `Share`; all four chart states including the F3 hold; the donut's range label, its own hold, every slice's count and share, and the five cases pinning its highlight |

Five of the seventeen findings were of a kind these files would have caught
before a browser did (F1, F6, F8, F10, F16); the sixth class — a jump that only
appears while a row is pending — is now impossible by construction rather than
by assertion.

### Re-measured in a real browser

Chromium against the running container, dev server proxying. Same instrument as
the first pass.

| # | Before | After |
| - | ------ | ----- |
| **F1** header vs row tracks at 1440 px | `337.5 46 62 104 285.6 311.5 84 0.01` vs `290.6 46 62 104 245.9 268.2 84 130` | **identical**, and every cell's `x` matches: 265 / 479 / 535 / 607 / 721 / 904 / 1104 / 1198 |
| **F1** row-to-row | actions cell 130 / 186 / 189 px by row state | fixed **200 px** on every row; `Delete and re-add` injected into a live row moved the Total column by **0 px** |
| **F1** all rows vs head | — | `true` at 1440 px and at 900 px |
| **F2** 24 h | one label per bar (~24) | 6 h cadence — `18:00 · 00:00 · 06:00`, 3 in this window's phase, 4 when a fourth boundary falls inside it |
| **F2** 7 d | — | 2 d cadence — `Aug 21 · 23 · 25 · 27`, 4 plus one clipped |
| **F2** 30 d | — | 6 d cadence — `Jul 30 · Aug 5 · 11 · 17 · 23`, **5**, which is plan §7.4's "~5" |
| **F4** refresh-all | 2 × `GET /lists`, 2 × `/telemetry` | unchanged and expected — 2 and 2, summary `4 refreshed · 1 failed, rejected lists included — 5 in all` |
| **F16** `/lists` at 900 px | `scrollWidth 964` vs `clientWidth 883` | **883 == 883**; the table now scrolls inside its card (`1094` in a `778` container) |
| **F6** phone tile labels | `Queries blocked · Percentage blocked · Cache hit rate` | `Blocked · Blocked · Cache hit` — read through `innerText`, which sees only the visible copy |
| **F17** tooltip | dark slab ~7 units from the dark card | raised surface, border and shadow; legible in both themes, dimming intact, `blocked %` still the served field |
| 390 px, both pages | — | `scrollWidth == clientWidth`; every control outside the drawer ≥ 44 px on both axes; screenshots retaken |
| lifecycle, unchanged | — | `fahTimers()` 5 on `/` and 1 on `/lists`, `fahUnion()` `['stats']` / `['list_refreshed']`, `fahChartBuilds()` 1 on load |

**F3 is proven by test, not by browser.** Reaching it needs `history.enabled`
switched off against a mounted page, which is a write to the container's
configuration; the four cases in `cards.test.tsx` cover the branch exactly,
including the one that must *not* blank on an ordinary range change.

### Changed-hunks re-review

Every hunk read back after the fact. Nothing found:

- `space` is a `uPlot.Axis.SpaceFn` and lives inside the options object already
  memoised on `(resolution, theme)`, so it adds no identity churn and cannot
  re-trigger m4.
- `empty` in `queries-over-time.tsx` covers `summary === null` as well, so the
  old first-load boot branch is preserved rather than replaced.
- `Tile` renders a bare string when `labelShort` is absent — five of the eight
  tiles gain no extra element — and `display: none` keeps the hidden copy out of
  the tab order and the accessibility tree.
- `sliceShare` guards `total <= 0`; `frequencyLabel` defaults, so neither domain
  table changes.
- Hiding `.l-rules .seg-legend` does not make colour the only signal: the mono
  partition line names each tier with its count, and the card title bar keeps
  the legend once.
- `min-width: min-content` is scoped to ≥ 768 px so a long unbreakable
  `last_error` URL cannot set the floor on the phone card layout.
- `StageBar`'s legend is untouched everywhere else — Cache state still renders
  `fresh · stale · expired · free`.

### Verdict

**PASS WITH DEFERRED FINDINGS** on the measured evidence above — *proposed, not
asserted*: the owner's instruction is that this task is not marked `PASS` or
`DONE` until they agree the measurements match the plan. The phase table still
reads `WAITING` and nothing here changes it.

The three blockers are closed and re-measured, and F16 — which the first pass
missed and which broke a stated plan invariant — is closed with them. What
remains open is six Minor/Nit rows, all cosmetic, none touching a figure, all
listed above with a reason.

**One measurement to judge rather than accept.** Plan §7.4 asks for "4 labels at
24 h". What ships is the artboard's *cadence* — one label every six hours — which
yields 3 or 4 depending on where the window's hour boundaries fall relative to
now. 7 d and 30 d land on ~5 as the plan asks. If the plan means exactly four at
every phase, that needs an explicit `splits` and a further edit.

The **Query types hover** the owner asked for during the review is built and
approved — as a legend-row highlight rather than the tooltip first requested,
for the reason recorded above. It is an addition neither artboard draws, so it
is listed as new work rather than as a finding closed.

---

## Second review pass — post-fix verification

**Scope.** Every hunk in the fix commit (`5509ae6`) against `2dc15c5`, the three
test files it adds, and the runtime behaviour those hunks reach. F1, F2 and F4
were reproduced from the running application rather than read off the text
above. Nothing was changed: no code, no plan, no other document, no git state.

**Instrument.** Chromium via Playwright against the running `fah-p506`
container (`fastadhunter:p5-05`), the dev server proxying at `localhost:5201`.
Axis labels read by intercepting `CanvasRenderingContext2D.fillText` — uPlot
paints them to canvas and the DOM carries none. Requests counted through a
`window.fetch` wrapper. Geometry from `getBoundingClientRect` and
`getComputedStyle`. Inventory at the time of measurement: **6 lists, 5
enabled** (one more than the first pass's five).

### Gates, re-run here

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | **328 passed / 26 files** — reproduces |
| `npm run build` | **59,869 B gzip (39.0 %)**, brotli **53,516 B** — reproduces byte-for-byte; postbuild assertions pass |
| Rust | not re-run, and correctly so: `git diff --name-only 383b904 5509ae6` touches `dashboard/`, `docs/` and `plan/` only. No `crates/`, no `Cargo.*` |

### F1, F2, F4 — reproduced directly

| # | What was measured | Result |
| - | ----------------- | ------ |
| **F1** | `gridTemplateColumns` off `.list-head` and all six `.list-row`s at 1600 px and at 1000 px | **identical on every element**, both widths: `265.3 46 62 104 224.5 244.9 84 200`. Cell `x` agrees head-to-row on all six cells (265 / 782 / 1016 / 1271 / 1365) |
| **F1** | the pending-state jump — Refresh clicked on `dead-source`, geometry read 120 ms later | actions cell **200 px before and during**; label changed to `refresh requested`; that row's Total column moved **0 px**; every other row's Total column unchanged at 1271. Closed by construction, not by tolerance |
| **F2** | x-axis labels, 1213 px plot | 24 h: **5 labels, 4 h cadence** — `16:00 · 20:00 · 00:00 · 04:00 · 08:00`. 7 d: **4**, 2 d — `Aug 21 · 23 · 25 · 27`. 30 d: **5**, 6 d — `Jul 30 · Aug 5 · 11 · 17 · 23`. Phone (292 px plot): 24 h gives **5** at 4 h. The wall of ~24 hourly labels is gone at every width |
| **F4** | refresh-all on the 5-enabled-list inventory, counted at `fetch` | `POST /lists/refresh` **1**, `GET /lists` **2**, `GET /telemetry` **2** — two waves 44 ms apart (t = 72464 and t = 72508). **The corrected V11 is what ships.** Summary rendered `4 refreshed · 1 failed, rejected lists included — 5 in all` over five result rows, so R17 holds |
| **F3** | not reproducible without writing `history.enabled` on the container | code path read instead: `empty` now covers `summary === null`, so the first-load boot branch survives; `QueryTypes` receives `loading` and holds on `recording && loading && slices.length === 0`. Four cases in `cards.test.tsx` cover both directions, including the one that must **not** blank on an ordinary range change |

**One recorded figure in the fix pass does not reproduce.** Its F2 row states
"6 h cadence … 3 in this window's phase, 4 when a fourth boundary falls inside
it". Measured here at the same 1213 px plot: **4 h cadence, 5 labels.** Both
readings are consistent with `space: dim / 5` — uPlot picks the increment from
the data's own span, which moves with the bucket count a response happens to
carry — and that is the point: **the label count is not pinned by anything.**
This confirms rather than contradicts the fix pass's own closing paragraph;
§7.4's "4 labels at 24 h" is met at some widths and phases and not at others.
Left as the owner's call, unchanged.

### The eight remaining dispositions, re-checked

| # | Claimed | Verified |
| - | ------- | -------- |
| **F6** | fixed | `innerText` at phone width reads `Blocked` · `Blocked` · `Cache hit`; the desktop copy is hidden, not removed |
| **F8** | fixed | `.l-rules .seg-legend` computes `display: none`; the Rules cell prints `58,879 dns · 0 url · 0 inactive` / `0 parse errors` **once** |
| **F10** | fixed | both domain tables head `Frequency`, Top clients heads `Share` |
| **F13** | fixed | `derive.sliceShare` is what the card calls; two cases in `derive.test.ts`, including `sliceShare(0, 0) === 0`. R16 is still inline and the correction note says so |
| **F16** | fixed | at a 1000 px viewport `documentElement.scrollWidth == clientWidth == 1000`; the table scrolls inside its card (`1094` in `895`). At 433 px both pages `scrollWidth == clientWidth` |
| **F17** | fixed | five `--tip-*` tokens in all three palette blocks; dark raises `--tip-surface` `#2b3846` above `--surface` `#18212c` and adds `--tip-border` `#47535f` |
| **F5, F7, F9, F11, F12, F14** | deferred | all still present exactly as described — the phone Cache card still prints `free 9,982`; the phone list card still orders header → meta → why → partition → actions; the footnote is still the DNS-only line and the resolution is still never stated; `/health` is still fetched twice at boot (both at t = 48 ms) |
| **F15** | doc-fixed | `ranges.ts` corrected; V11 and V14 struck through and restated |

### The invariants asked about

| Invariant | Result |
| --------- | ------ |
| Sketch is the visual source of truth | held, with one new exception — see **N1** |
| API provenance / §8.2 boundary | held. The fix pass adds no figure: `sliceShare` **is** R13 moved, `labelShort` and `frequencyLabel` are labels |
| Route-scoped subscriptions and refresh timers | held. `fahTimers()` **5** on `/`, **1** on `/lists`, **0** on `/policies`; `fahUnion()` `['stats']` / `['list_refreshed']` / `[]`; socket `open` → `open` → **`closed`** |
| Shared endpoint refcounting | held. Dashboard → Lists issued **exactly one** request, `GET /telemetry`. No second `GET /lists` |
| Chart option memoisation | held. `useMemo` keyed on `[resolution, theme]`; the new `space` closure lives inside that object. `fahChartBuilds()` **3 → 3** across 10 s of `stats` pushes |
| uPlot lazy loading | held. `uPlot.esm-*.js` is the only asset naming uPlot; the postbuild assertion runs and the build exits 0 |
| No forbidden strings | held. Neither `dist/` nor `src/` contains `pi-hole` / `pihole`. `allow` survives only as the API field name in the R5 sum at `tiles.tsx:100` |
| 44 px touch targets | **broken — see N1** |

---

### N1 · Major · The list enable/disable switch is a 32 × 18 px touch target

`dashboard/frontend/src/styles/components.css:1332-1342`

`.switch` is a hard `width: 32px; height: 18px` with no phone override — the
rule sits **after** the `@media (max-width: 767px)` block closes at line 1328,
so the figure is the same at every width. The `<input type="checkbox">` inside
it is `position: absolute; inset: 0` and inherits the same box; the `.l-on`
wrapper measures 32 × 22.

Measured at a phone viewport, one per list row, six rows:

| control | w × h | in the drawer? |
| ------- | ----: | -------------- |
| `label.switch` | **32 × 18** | no |
| its `input[type=checkbox]` | **32 × 18** | no |
| every `button` on both pages | ≥ 44 × 44 | — |

**This contradicts two recorded statements.** V17 reads "No interactive control
outside the drawer measured under 44 px on either axis at 390 px", and the fix
pass's browser table repeats "every control outside the drawer ≥ 44 px on both
axes". The task file's acceptance criteria state it plainly: *"Touch targets on
interactive controls are at least 44 px."* The Dashboard is clean — 15
interactive elements, none under 44 px — so the violation is Lists-only, and it
is that page's only stateful control.

**Why both measuring passes missed it.** The switch is neither a `button` nor an
`a`; a sweep over those two element types returns nothing. Reaching it needs
`input` and `label.switch` in the selector.

**`MobileLists.dc.html` draws it at 32 × 18 — and that does not settle it.**
This review file's own deviations table already made the argument and applied it
twice: *"Touch targets grown past the artboards' 22 px tile footer and 18 px
trailing links — the acceptance criterion states 44 px; a drawn height is not a
measurement (phase constraint 8)."* The same reasoning was not carried to the
switch the same task drew. Phase constraint 8 is explicit that sketch figures
are not measurements.

**Impact.** An 18 px-tall target is about two fifths of the guideline in the
axis that matters most for a thumb, and it sits inside a 44 px row — so a miss
lands on inert text beside it rather than on nothing, which reads as a tap that
did nothing.

**Smallest correct fix, and it moves no drawn pixel.** Expand the hit area, not
the pill: a `.switch::before` with `content: ''; position: absolute; inset:
-13px -6px;` gives 44 × 44 while the 32 × 18 track and its 14 px knob render
exactly as the artboard draws them. `.switch` is already `position: relative`.
One rule.

**Fix before `DONE`.** It is a stated acceptance criterion and a recorded PASS
that does not reproduce — the same class as F4, one severity up because here the
behaviour is wrong, not merely the sentence describing it.

### N2 · Nit · A range change paints the previous range's bars under the new range's axis labels

`dashboard/frontend/src/pages/dashboard/queries-over-time.tsx:52-86`

The options memo is keyed on `[resolution, theme]` and flips the moment a chip
is clicked; `data` is keyed on `[summary]` and lags by the `/history/summary`
round trip. For that one interval the old series is drawn through the new
range's formatter.

Captured off `fillText`, 30 d → 24 h at a 1213 px plot:

```text
frame 1   00:00  00:00  00:00  00:00  00:00      x = 55 281 507 733 959
          (the 30 d bucket positions and the 30 d y-scale, hour-formatted)
frame 2   16:00  20:00  00:00  04:00  08:00      x = 132 350 568 787 1005
```

`axisTimeLabel` renders a UTC-midnight day boundary as `00:00` under the hour
formatter, so five identical labels are the visible symptom. Reproduced at phone
width too (7 d → 24 h, four × `00:00`).

**Pre-existing, not introduced by the fix pass** — the earlier code also fell
through to render with stale data on a range change, and F3's fix deliberately
keeps the bars up (`cards.test.tsx` pins that). What is new is that the
behaviour is now stated as intended, which makes the mislabelled frame worth
recording. Sub-second on a LAN; longer on a slow link.

**Deferrable.** Closing it means holding the axis on the old resolution until
the response lands, which couples the memo to in-flight state — more moving
parts than a sub-second flash is worth. Recorded so `p5-08` does not rediscover
it when it reuses this chart.

### N3 · Nit · `overflow-x: auto` on `.bd.lists-body` also makes it scroll vertically

`dashboard/frontend/src/styles/components.css:1090-1096`

CSS computes a non-`visible` value on one axis into `auto` on the other.
Measured: `overflowX: "auto"`, **`overflowY: "auto"`**. Harmless today —
`scrollHeight == clientHeight` (1707 = 1707) because the container has no height
cap, and the only absolutely positioned descendants are the switch's own input
and knob, both inside their own positioned label.

It does make the card a scroll container and a block formatting context. Worth
knowing before anything that paints outside the padding box goes inside it — a
dropdown, a positioned tooltip, a focus ring on an edge cell. No change asked
for.

### N4 · Nit · F17 removed two literal colours and added one

`tokens.css`' header allows no exception: *"no rule outside this file may name a
literal colour."*

| Where | Literal | Whose |
| ----- | ------- | ----- |
| `components.css:569` | `box-shadow: 0 4px 14px rgb(0 0 0 / 35%)` | **added by the F17 fix** |
| `components.css:1363` | `.switch i { background: #fff }` | p5-06's own, unfixed |
| 7 further sites | `#fff`, `rgba(0, 0, 0, …)` | p5-05's, present at `383b904` |

The F17 entry reads "two literal colours removed from `components.css`", which
is true and incomplete: the same edit introduced a third and left p5-06's own
fourth in place. Either the rule takes a stated shadow/knob exception or these
become tokens — a one-line decision either way, and not this task's blocker.

---

### Verdict — second pass

**BLOCKED**, on one new Major.

Everything the first pass blocked on is genuinely closed, and closed under
independent measurement rather than under the fix pass's own account of it:
**F1, F2, F3, F6, F8, F10, F13, F16, F17** all verify, F4's corrected V11
reproduces exactly (1 / 2 / 2), and the six deferred rows are present and
unchanged. Every invariant listed for re-checking holds — API provenance, the
§8.2 boundary, route-scoped timers and subscriptions, shared-endpoint
refcounting, chart memoisation under the `stats` push, the uPlot chunk split and
the absence of forbidden strings. The three new test files pin what they claim
to, including the one invariant jsdom cannot compute. No fix weakened or
bypassed anything.

What blocks it is **N1**: the per-row enable/disable switch is a 32 × 18 px
touch target at every width, against an acceptance criterion that says 44 px and
against a V17 row recording that no such control exists. One CSS rule closes it
without moving a drawn pixel.

N2–N4 are Nits, fine to defer or to take in the same pass. The open judgement
call the fix pass raised — what the 24 h label count should actually be — stands
unresolved and is the owner's, not a defect.

**Re-run after N1:** the 44 px sweep at phone width with `input` and
`label.switch` in the selector, `npm run test`, `npm run build`.

---

## Second fix pass — N1–N4 and the 24 h label count

Approved by the owner. Four findings closed and one open decision settled. **No
plan, task file or other repository document was changed**, no deferred finding
(F5, F7, F9, F11, F12, F14) was touched, and nothing was committed.

### Changed

| # | Change | Files |
| - | ------ | ----- |
| **N1** | `.switch::before` — a 44 × 44 pseudo-element centred on the pill. The drawn 32 × 18 track and its 14 px knob are untouched; only the target grows | `styles/components.css` |
| **N2** | the chart's axis now takes its resolution from **the response** (`plottedResolution`), not from the chips, so the axis and the bars always describe the same data | `pages/dashboard/ranges.ts`, `queries-over-time.tsx` |
| **N3** | the horizontal scroller moved off `.bd.lists-body` and onto `.lists-table`; the per-row floor moved with it, from the container to `.list-head, .list-row` | `styles/components.css` |
| **N4** | `--tip-shadow` and `--switch-knob` added to all three palette blocks; the two literals in `components.css` replaced by them | `styles/tokens.css`, `components.css` |
| **§7.4** | `hourSplits` — four hour-aligned x splits, computed from the window rather than chosen by uPlot from a hint | `charts/stacked-bars.ts` |

**N1 — why a pseudo-element and not a bigger control.** `MobileLists.dc.html`
draws the pill at 32 × 18 and §2 makes that the visual authority; the
acceptance criterion asks for a 44 px *target*. Both are satisfiable at once
because they are different things, which is the same reasoning the first fix
pass used on the 22 px tile footer and the 18 px trailing links. The box is
centred on the pill rather than inset from it, so it stays 44 px whatever the
pill becomes.

**N2 — why the response and not a loading gate.** Blanking the plot for the
round trip would undo F3's "keep the bars up while a range with data is
refetched", which `cards.test.tsx` pins. Reading the resolution off the
response instead leaves that behaviour alone and makes the mismatch
unrepresentable: both halves flip together when the response lands. The memo
key is unchanged in shape — `[resolution, theme]` — so the rebuild count is what
it was.

**N3 — what CSS actually allows.** A horizontal scroll container is a scroll
container on **both** axes: a `visible` companion computes to `auto`, and `clip`
computes to `hidden` in the same position, so one axis alone cannot be asked
for. What can be done is put it on the smallest element that needs it. On the
card body it also swept in the footnote — prose, which was scrolling sideways
with the rows; on the table it holds only the head and the rows, whose height is
their content's, so the vertical axis is inert and now provably so.

**§7.4 — why a hint could not deliver a count.** `space` tells uPlot how much
room a label wants; uPlot then picks the nearest increment from its own table
and emits however many fall inside the window. The count therefore moved with
the response's bucket count and with where the window's hour boundaries sat —
3, 4 or 5 for the same range at the same width, which is what the first fix
pass measured as "6 h cadence" and this review measured as "4 h cadence".
`hourSplits` anchors on the window's first whole hour and steps by a quarter of
it, rounded to the hour: **four labels, always, on the clock.** 7 d and 30 d keep
`space: dim / 5` untouched, which the plan asks only "~5" of.

### Gates

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | **345 passed / 28 files** (was 328 / 26 — two new files, 17 new cases) |
| `npm run build` | **60,067 B gzip against 153,600 B — 39.1 %**, brotli 53,727 B. Was 59,869 B / 39.0 %; **+198 B** |
| postbuild assertions | pass |
| Rust | not re-run. No `crates/` or `Cargo.*` path was touched |

### New tests

| File | Cases | Pins |
| ---- | ----: | ---- |
| `styles/literal-colours.test.ts` | 4 | an **allowlist**, not a ban: the seven inherited literals in `base`/`components`/`layout` must stay exactly seven and in place, so a new one fails here rather than being noticed a review later. Plus both new tokens defined in all three palette blocks — a token in one block only is the other half of the same mistake |
| `pages/dashboard/ranges.test.ts` | 3 | the chart follows the response's resolution while a range change is in flight, falls back to the chip only when nothing is plotted, and agrees with the chip once the response lands |
| `charts/stacked-bars.test.ts` (+6) | 6 | four labels for **all 24 opening hours** of a window, the artboard's own `10:00 · 16:00 · 22:00 · 04:00`, survival of uPlot's half-slot bar padding, every split on a whole hour, no sub-hour step on a short window, and a zero-width window |
| `styles/grid-tracks.test.ts` (rewritten in part) | +4 | the scroller is the table and **not** the card body, no height is imposed on it, the row floor is on the rows, and the pill stays 32 × 18 while its target is 44 × 44 |

### Re-measured in a real browser

Same instrument as before — Chromium against `fah-p506`, dev server proxying.
Viewport figures are CSS pixels.

| # | Check | Result |
| - | ----- | ------ |
| **N1** | effective target, both pages at 390 px, `input` and `label.switch` in the selector | **zero controls under 44 px** on either page. `.switch::before` measures 43.99 × 43.99 |
| **N1** | does it steal clicks | all four corners of the 44 × 44 box resolve to `LABEL.switch`. At 1440 px **no control on the page loses its own centre** — the sweep returned an empty list. The metadata row's `every` and `last` cells still own theirs |
| **N1** | does the enlarged target actually work | a click **20 px diagonally out from the pill centre** — outside the 32 × 18 pill — flipped `local-extra` `false → true`. Restored to `false` afterwards |
| **N1** | the drawing | pill still 32 × 18 at every width; the `On` column is a 46 px track and holds the 44 px box |
| **N2** | 30 d → 24 h, every axis frame captured off `fillText` | **one frame only**: `14:00 · 19:00 · 00:00 · 05:00`. The `00:00 · 00:00 · 00:00 · 00:00 · 00:00` frame is gone. 24 h → 7 d and 7 d → 30 d likewise show one frame each |
| **N2** | memoisation | `fahChartBuilds()` 1 → 2 on hour→day, **2** on day→day (memo holds), 3 on day→hour. **3 → 3** across 10 s of `stats` pushes |
| **§7.4** | label counts at a 1053 px plot | 24 h **4** (`14:00 · 19:00 · 00:00 · 05:00`, whole hours), 7 d **4** (`Aug 21 · 23 · 25 · 27`), 30 d **5** (`Jul 30 · Aug 5 · 11 · 17 · 23`) — the plan's four, and 7 d / 30 d unchanged |
| **N3** | `/lists` at 900 px | page `883 == 883`, no body scroll — **F16 unchanged**. Table scrolls inside itself, `1094` in `778`. Head and all six rows still resolve identical tracks — **F1 unchanged** |
| **N3** | the implicit second axis | `.bd.lists-body` now computes `overflow: visible / visible` — no longer a scroll container. `.lists-table` is `auto / auto` with `scrollHeight == clientHeight` (541 = 541), so nothing scrolls or clips vertically |
| **N3** | the footnote | now **outside** the scroller and full width (778 px); it used to scroll sideways with the rows |
| **N4** | tokens in both themes | `--tip-shadow` `rgb(0 0 0 / 35%)` and `--switch-knob` `#ffffff` resolve in light, dark and after a toggle back |
| **N4** | rendered appearance | the raised tooltip computes `rgba(0, 0, 0, 0.35) 0px 4px 14px` — byte-identical to the literal it replaced, so F17's look is preserved. `blocked %` is still the served field (`12.0`) |
| lifecycle | unchanged | `fahTimers()` **5** on `/`, `fahUnion()` `['stats']`, socket `open` |
| both pages at 390 px | unchanged | `scrollWidth == clientWidth` on each |

**One measurement worth stating so it is not read as a defect.** Resizing the
window from 1440 px to 390 px *while a chart tooltip is pinned* leaves the
overlay at its old x and the document reports 742 px of scroll width until the
next cursor event. Loading either page at 390 px, or raising a tooltip there,
gives `416 == 416`. It is a stale absolutely-positioned overlay across a live
viewport resize, not a layout defect, and no dispositions rest on it.

### Still open — the Lists table's own width

> **Superseded.** Settled by the third fix pass and the documentation edit
> below. The measurement in this section stands; the sentence about
> `visual-system.md` is **wrong as written** and is struck through — see
> §"A correction to this document".

Not a finding and not in this pass's scope; recorded because it was measured
here.

`.lists-table` cannot be narrower than **1094 px** — that is the sum of the
eight declared minimums (`150 + 46 + 62 + 104 + 160 + 190 + 84 + 200 = 996`),
seven 10 px gaps and 28 px of row padding, and it matches the measured
`scrollWidth` exactly. The container reaches 1094 px at about a **1365 px**
viewport, so the table scrolls below that. ~~`visual-system.md` §Responsive
sanctions that scroll for **768–1199 px** and asks for the full grid at ≥ 1200,
so the band 1200–1365 px scrolls where the document says it should not.~~
**WRONG AS RECORDED.** That clause belongs to the `< 768 px` row, where Lists
renders as cards; the sentence that governs is unconditional and the
implementation satisfies it. There is no contradiction — only an undocumented
figure, which the edit at the end of this file now supplies.

The 200 px actions track is the largest single column and is F1's fix — it has
to stay content-independent, so shrinking it is not on offer. What is:

| | Change | Fits from |
| - | ------ | --------- |
| a | leave it — **what ships** | ~1365 px |
| b | trim the soft floors: Status 160 → 110, Rules 190 → 150, List 150 → 130, Last refresh 104 → 78 | ~1240 px |
| c | b, plus merging `Every` and `Last refresh` into one cell as the phone card already does (−72 px) | ~1165 px |

(b) needs no structural change and no artboard deviation; (c) departs from
`Lists.dc.html`'s drawn column set. **Neither was applied** — the owner was
offered them during this pass and asked for the scoped fixes only.

### Verdict — after the second fix pass

**PASS WITH DEFERRED FINDINGS** on the measured evidence above — *proposed, not
asserted*: the owner's instruction is that this task is not marked `PASS` or
`DONE` until they agree the measurements match the plan. The phase table still
reads `WAITING` and nothing here changes it.

N1, the only blocker, is closed and proven by a click landing 20 px outside the
pill. N2, N3 and N4 are closed with it, and the 24 h label count is now a
property of the code — four, at whole hours, for every one of the 24 possible
window phases — rather than of whatever uPlot inferred from a hint. Every
invariant re-checked in the pass before this one still holds, measured again
here: route-scoped timers and subscriptions, shared-endpoint refcounting, chart
memoisation under the `stats` push, F1's grid tracks, F16's page-scroll
freedom, and F17's rendered appearance.

What remains open is **six Minor/Nit rows — F5, F7, F9, F11, F12, F14** — all
cosmetic, none touching a figure, each listed above with its reason, plus the
table-width question in the section above, which is a judgement rather than a
defect.

---

## Third fix pass — the Lists table's own width (option b)

Approved by the owner: trim the four soft column floors, keep the internal
scrollbar wherever the desktop table cannot fit, do **not** touch the 200 px
actions track, do **not** merge `Every` and `Last refresh`. Validate against
`visual-system.md` §Responsive and report the measured minimum if the 1200 px
contract cannot be met.

**It cannot. The Lists table's own measured minimum is 1247 px.** Details below.

> **Terminology, fixed here and used the same way everywhere after it.**
> **1247 px is the measured minimum viewport at which the Lists table fits
> without an internal scrollbar.** It is a property of that one table's eight
> columns, not a breakpoint. The application's breakpoints are unchanged and
> remain `visual-system.md` §Responsive's — ≥ 1200 px, 768–1199 px, < 768 px.
> Where the text below says "the 1200 px contract", it means the expectation
> that a desktop-layout page needs no internal scrolling, not the breakpoint
> itself.

### Changed

| Track | Floor before | after |
| ----- | -----------: | ----: |
| List | `minmax(150px, 1.3fr)` | **130px** |
| Last refresh | `104px` | **78px** |
| Status | `minmax(160px, 1.1fr)` | **110px** |
| Rules | `minmax(190px, 1.2fr)` | **150px** |
| actions | `200px` | **unchanged** |

The `fr` weights are untouched, so nothing moves at a width where the row
already fits — the trim only lowers where the row stops fitting. Row floor:

```text
130 + 46 + 62 + 78 + 110 + 150 + 84 + 200 = 860
+ 7 gaps × 10 = 70   + row padding 28     = 958      (was 1094)
```

**One further change the validation forced.** At the Status column's new floor
the `dead-source` row's `last_error` painted **35 px outside its own cell**: the
string carries a bare URL, `(http://172.17.0.99/nope.txt):` is a 145 px token
with no break opportunity, and 145 does not fit in 110. `.l-status .note` gains
`overflow-wrap: anywhere`. It is the right rule for an address — read, not
scanned — and it also covers a longer URL than this one, which would have spilled
at the old 160 px floor too. The `failed` row grows from 233 px to 267 px tall
because the URL now wraps instead of overflowing; no other row changes height.

### Measured — `/lists`, both themes, chrome overhead 271 px

| viewport | `clientWidth` | container | table floor | internal scrollbar | page body scrolls |
| -------: | ------------: | --------: | ----------: | ------------------ | ----------------- |
| 1200 px | 1183 | 912 | 958 | **yes**, 46 px short | no |
| 1240 px | 1223 | 952 | 958 | **yes**, 6 px short | no |
| **1247 px** | 1230 | 958 | 958 | **no** — first width that fits | no |
| 1300 px | 1283 | 1012 | — | no | no |
| 1365/1366 px | 1366 | 1094 | — | no | no |

Dark and light are identical at every width — geometry does not read the
palette. Before this pass the same table first fitted at ~1365 px, so the trim
moved the threshold down **118 px**.

### Acceptance, item by item

| Asked | Result |
| ----- | ------ |
| no page/body horizontal scroll at ≥ 1200 px | **holds** — `scrollWidth == clientWidth` at 1200, 1240, 1247, 1300 and 1366, both themes |
| table fits without an internal scrollbar wherever the documented full grid applies | **not met, by 46 px at 1200 px.** The table fits from 1247 px up; below that it scrolls inside its card while the page keeps its desktop layout. Accepted and documented rather than closed — see below |
| below the threshold, scrolling stays inside the table container | **holds** — at 1200 px, 958 px of table in a 912 px scroller, page body clean. Re-checked at 900 px: 1094-era behaviour preserved, `883 == 883` |
| header and all row columns aligned | **holds** — head and all six rows resolve identical tracks at every width measured; the Total column's `x` is one value across head and rows (901 at 1200, 954 at 1300, 1037 at 1366) |
| normal, pending and rejected rows keep identical tracks | **holds** — measured at 1200 px, the tightest: tracks equal, actions cell 200 × 18 and Total `x` = 901 in all three states, including a live `refresh requested` and an injected `Delete and re-add` |
| no action wrapping or vertical layout jump | **holds** — the actions cell is 18 px tall in every row and every state; row heights are byte-identical across the three snapshots (81 / 81 / 233 / 64 / 81 / 81) |
| readability not harmed | **one harm found and fixed** — the `last_error` spill above. After the wrap rule **no cell on any row overflows** at 1200 px |

### The 1200 px contract — measured, not met

`visual-system.md` §Responsive asks for the full grid at ≥ 1200 px. At 1200 px
the card gives the table **912 px** and the row's floor is **958 px** — short by
**46 px**. Every remaining source of 46 px was excluded by the instruction or by
an earlier finding:

| Where 46 px could come from | Why not |
| --------------------------- | ------- |
| the 200 px actions track | F1's fix. It must stay content-independent, or the header and the rows disagree again and a pending row jumps sideways. Excluded by the owner |
| merging `Every` and `Last refresh` | excluded by the owner; departs from `Lists.dc.html`'s drawn column set |
| trimming the four soft floors further | they are already at what their content needs — the Status trim to 110 px is what pushed a URL out of its cell, and the fix was to break the URL, not to give the column back its width |
| dropping a column | `Lists.dc.html` draws all seven; §2 makes the artboard the authority on structure |

**Reported rather than invented around, as instructed. The measured minimum
viewport at which the Lists table fits without an internal scrollbar is
1247 px** (1230 px of `documentElement.clientWidth`, 958 px of container).
Between 1200 and 1247 px the table scrolls inside its own card; the page body
never does, and the page is in its desktop layout throughout.

That figure is the table's, not the application's. **No breakpoint moves** —
what is undocumented is the table's own floor, which the edit recorded at the
end of this file supplies. Nothing was changed in this pass; the wording above
about the document disagreeing with the code was itself wrong and is corrected
in §"A correction to this document".

### Gates

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | **347 passed / 28 files** (was 345 — two new cases) |
| `npm run build` | **60,082 B gzip against 153,600 B — 39.1 %**, brotli 53,710 B. Was 60,067 B; **+15 B** |
| postbuild assertions | pass |
| Rust | not re-run. No `crates/` or `Cargo.*` path was touched |

### New tests

| Case | Pins |
| ---- | ---- |
| `grid-tracks.test.ts` — "keeps its own floor under what the full grid can give it" | the floor arithmetic, to **958**. A widened track fails here, with the 1247 px consequence written beside it, so the band cannot grow again unnoticed |
| `grid-tracks.test.ts` — "breaks a bare URL rather than painting outside its column" | `.l-status .note { overflow-wrap: anywhere }`, without which the narrower Status floor spills a `last_error` into the gutter |

---

## Final verification at the Lists table's 1247 px minimum

The owner accepted **1247 px** as the measured width at which the Lists table
fits without an internal scrollbar — a figure belonging to that table, not a new
application breakpoint — and asked for
one last pass before any documentation moves. **No code was changed in this
pass.** Column widths and table structure are exactly as the third fix pass left
them.

### 3 widths × 2 themes × 3 row states — 18 measurements, all clean

`rejected` is the widest action label (`Delete and re-add`) injected into a live
row, `pending` a real `POST /lists/{id}/refresh` on `dead-source`.

| viewport | container | internal scrollbar | body scrolls | tracks equal | misaligned cells | text overflow | actions track | actions height | row heights |
| -------: | --------: | ------------------ | ------------ | ------------ | ---------------: | ------------: | ------------: | -------------: | ----------- |
| 1247 px | 958 | **none** | no | yes | **0** | **0** | 200 px | 18 px | 81/81/267/64/81/81 |
| 1250 px | 962 | **none** | no | yes | **0** | **0** | 200 px | 18 px | 81/81/267/64/81/81 |
| 1300 px | 1012 | **none** | no | yes | **0** | **0** | 200 px | 18 px | 81/81/215/64/81/81 |

Dark and light are identical at every width and in every state — geometry does
not read the palette.

- **Alignment** was checked cell by cell, not by the grid template alone: all
  eight cells of all six rows sit at the header's own `x` at every width. Zero
  disagreements out of 8 × 6 × 3 × 3 comparisons.
- **No layout jump.** Row heights are byte-identical across `normal`,
  `rejected` and `pending` at each width. The `dead-source` row is 267 px at
  1247/1250 and 215 px at 1300 — that is the wrapped `last_error` reflowing as
  the Status column widens, which happens between widths, never between states.
- **The actions track never moves**: 200 px on the header and on every row, in
  every state, at every width.
- **No text overflow anywhere**, which is the `overflow-wrap` rule from the
  third pass holding at the tightest column widths the design now allows.

### A correction to this document

The §"Still open — the Lists table's own width" section above says
`visual-system.md` §Responsive "sanctions that scroll for **768–1199 px**".
**It does not.** Read at source, the table reads:

| Width | Behaviour |
| --- | --- |
| ≥ 1200 px | full grid, sidebar expanded |
| 768–1199 px | halves become full width, sidebar collapses to icons |
| < 768 px | single column, sidebar is an overlay drawer, tables scroll inside their own container |

followed by: *"The page body never scrolls horizontally. Wide tables and charts
scroll inside themselves."*

Three consequences, and they make the gap much smaller than that section stated:

1. The "tables scroll inside their own container" clause belongs to **< 768 px**,
   where Lists renders as cards and has no table — so it never applied here.
2. The **unconditional** sentence under the table is the one that governs, and
   the implementation satisfies it exactly: the page body never scrolls, the
   wide table scrolls inside itself.
3. The ≥ 1200 px row is about the **page** grid and the sidebar, not about a
   table fitting. Measured at 1200 px: sidebar **230 px and expanded with
   labels**, the two half-width cards side by side at `x` 250 and 714, no body
   scroll. **That row is accurate as written and does not need to change.**

So there is no contradiction to repair — only an undocumented figure.

### Proposed documentation edit — ~~not applied, awaiting approval~~ **superseded**

> The owner approved the edit but rewrote it shorter and more neutral. **What
> shipped is the version in §"Documentation edit — applied" at the end of this
> file**; the draft below is kept only because the reasoning under it is what
> the decision rested on.

One sentence appended to the existing paragraph in
`docs/dashboard/visual-system.md` §Responsive. The breakpoint table is untouched.

```diff
 The page body never scrolls horizontally. Wide tables and charts scroll inside
-themselves.
+themselves. The Lists table is the one that reaches that limit on a desktop:
+its eight columns floor at 958 px of card, which a 1247 px viewport is the
+first to supply, so between 1200 and 1247 px the page grid is already full
+while the table still scrolls inside its card.
```

Why this and nothing larger:

| Alternative | Why not |
| ----------- | ------- |
| move the ≥ 1200 px row to ≥ 1247 px | that row is about the page grid and the sidebar, both of which are correct at 1200 px — moving it would make an accurate row wrong |
| add a fourth breakpoint row | the rows describe page layout; one table's own floor is not a breakpoint |
| say nothing | the figure is measured, it is load-bearing for `p5-07`'s tables, and a reader cannot otherwise tell which table the unconditional sentence is about |

No other document needs an edit. `plan/wip/phase5/p5-06-dashboard-and-lists-plan.md`
§9.3 says "the page body never scrolls horizontally at any width", which is
measured true at every width in this pass and in the two before it.

### Documentation edit — **applied**

Approved by the owner, in the owner's own wording — shorter and more neutral
than the version proposed above, and it does not pin the sentence to a figure
that a padding change would move.

`docs/dashboard/visual-system.md` §Responsive, one paragraph. **The breakpoint
table is untouched.**

```diff
 The page body never scrolls horizontally. Wide tables and charts scroll inside
-themselves.
+themselves. The Lists table reaches that limit on desktop: its eight columns
+floor at 958 px of card width, so between 1200 and 1247 px the page grid is
+already in its desktop layout while the table scrolls inside its card.
```

The settled behaviour, stated once:

| Width | Layout | Lists table |
| ----- | ------ | ----------- |
| ≥ 1247 px | desktop | fits, no scroll |
| 1200–1246 px | desktop | scrolls inside its card |
| < 1200 px | the responsive rules above, unchanged | — |

**This closes the table-width question.** No other repository document was
changed, and no code changed in this pass or the one before it.

### Wording audit — 1247 px is the table's figure, not a breakpoint

Every mention of the figure in this repository was re-read and made to say the
same thing. **The application's breakpoints are unchanged and remain
`visual-system.md` §Responsive's: ≥ 1200 px, 768–1199 px, < 768 px.**

| Where | State |
| ----- | ----- |
| `docs/dashboard/visual-system.md` §Responsive | correct as shipped — the sentence names the Lists table as its subject and the breakpoint table is untouched |
| `plan/wip/phase5/p5-06-dashboard-and-lists-plan.md` §9.3 | no change needed. It quotes the three breakpoints, which did not move, and asserts "the page body never scrolls horizontally at any width", measured true throughout |
| `plan/wip/phase5/p5-06-dashboard-and-lists.md` | never mentions the figure |
| this review file | four places rewritten — see below |
| `styles/grid-tracks.test.ts` | the pinning case's comment reframed. Comment only; no assertion, no behaviour. Gates re-run |

Rewritten in this file:

| Was | Now |
| --- | --- |
| "the measured minimum is 1247 px" | "**the Lists table's own** measured minimum is 1247 px", with a terminology note fixing the phrase for everything after it |
| "either `visual-system.md`'s **full-grid threshold moves to 1247 px**, or …" | "that figure is the table's, not the application's. **No breakpoint moves**" |
| heading "Final verification at **the accepted 1247 px threshold**" | "Final verification at **the Lists table's 1247 px minimum**" |
| "the owner accepted 1247 px as **the desktop full-grid threshold**" | "as the measured width at which the Lists table fits without an internal scrollbar — a figure belonging to that table, not a new application breakpoint" |

Two stale passages were struck rather than deleted, following this file's own
convention for a superseded claim (V11, V14): the §"Still open" paragraph that
misread `visual-system.md`, and the first draft of the documentation edit, which
the owner replaced with a shorter wording.

**Settled, in one form of words:**

| Width | Application layout | Lists table |
| ----- | ------------------ | ----------- |
| ≥ 1247 px | desktop | fits, no internal scrollbar |
| 1200–1246 px | desktop | scrolls inside its card |
| < 1200 px | `visual-system.md` §Responsive, unchanged | — |

Gates after the comment change: `npm run typecheck` clean, `npm run test`
**347 passed / 28 files**, `npm run build` **60,082 B gzip (39.1 %)**, brotli
53,710 B — all unchanged.

---

## Fourth fix pass — glyph row actions, and the Rules column's share

Approved by the owner after reviewing the page at ~1200 px, where the row still
scrolled and `Remove` was clipped. Two changes, one of them the owner's
suggestion.

### Changed

| # | Change | Files |
| - | ------ | ----- |
| 1 | the three row actions become **44 × 44 icon buttons** — `refresh`, `edit`, `trash`, and `restore` on a `rejected` row — each with `aria-label` and `title`. The actions track goes **200 px → 132 px** | `pages/lists/list-actions.tsx`, `assets/sprite.svg` (+3 symbols), `styles/components.css` |
| 2 | the Rules column takes the **largest `fr` weight** (List 1, Status 1, Rules 2) | `styles/components.css` |

**Why glyphs are structural here, not decoration.** As labels the three cost
200 px of an eight-column row, which is what put an internal scrollbar on the
table at every desktop width below 1247 px. They also varied by state — 130 px
normally, 186 px pending, 189 px `rejected` — which is the variance F1's fixed
200 px track existed to absorb. Three tiled 44 px targets are **132 px and
cannot vary at all**, so F1 is closed by construction rather than by a
clearance figure. The row floor drops **958 → 890 px**.

**Why the `fr` weight, and why it is free.** The three-line wrap of
`900 dns · 0 url · 0 inactive · 0 parse errors` was never a width shortage:
Rules' 150 px minimum already exceeds its proportional share, so under the old
weights every pixel of slack went to List and Status while Rules stayed pinned
at its floor. Raising its weight redistributes rather than adds — **the floor is
unchanged by it** — and the line drops to two rows at every desktop width.

**Pending is now the same button in a busy state**: the refresh glyph spinning,
disabled, `aria-label` reading `Refresh requested for <id>`, `animation: none`
under `prefers-reduced-motion`. No second label, therefore no second width.

**The confirmations are untouched and were re-verified live.** Clicking the
trash on `gate-list` opened `Remove gate-list?` — *"The list is dropped from
`[[rules.lists]]` and its cached copy is deleted, so its 900 rules stop serving
at the next compile."* — with `Cancel` / `Remove`; Cancel closed it and all six
rows remained. `Delete and re-add` keeps its own dialog and its own reasoning
verbatim. **A glyph never performs a destructive action without the sentence
that names the consequence.**

### Measured — 4 widths × 2 themes × 3 row states

`rejected` renders the `restore` glyph; `pending` is a real
`POST /lists/{id}/refresh`. Row states were exercised at 1200, 1247 and 1300;
390 px covers normal and pending (the phone card has no header to align).

| viewport | table floor | internal scrollbar | body scrolls | tracks equal | misaligned cells | text overflow | Rules cell | partition lines | actions | icon button |
| -------: | ----------: | ------------------ | ------------ | ------------ | ---------------: | ------------: | ---------: | --------------: | ------: | ----------- |
| **1200 px** | 890 (fits 912) | **none** | no | yes | **0** | **0** | 172 px | **2** | 132 px | 44 × 44 |
| 1247 px | 890 | none | no | yes | 0 | 0 | 218 px | 2 | 132 px | 44 × 44 |
| 1300 px | 890 | none | no | yes | 0 | 0 | 254 px | 2 | 132 px | 44 × 44 |
| 390 px | card layout | — | no (`373 == 373`) | n/a | — | 0 | — | 2 | full width | 99 × 44 |

Dark and light are identical at every width and in every state. **Zero controls
under 44 × 44** at 390 px outside the drawer. Row heights are byte-identical
across the three states at each width, so no state introduces a jump; ordinary
rows are now **64 px** rather than 81, because the partition line lost a row.

Before this pass, for comparison: at 1200 px the floor was 958 in a 912 px
container — a scrollbar, `Remove` clipped, Rules 150 px and its partition line
on **three** rows.

### The 1200–1247 exception is gone, and so is its documentation

The table now fits from the 1200 px breakpoint up, so the sentence added to
`docs/dashboard/visual-system.md` §Responsive one pass earlier has been
**reverted**. `git diff` on that file is empty: it ends this task exactly as it
began it, and the unconditional *"Wide tables and charts scroll inside
themselves"* covers the sub-1200 case as it always did.

The `grid-tracks.test.ts` case that pinned the old floor now pins the new one
**against the width the breakpoint supplies** — `890 ≤ 1200 − 271 − 17` — so a
track widened later fails the gate with the reason beside it rather than
quietly bringing the band back.

### One artboard discrepancy, declared and **not** acted on

`MobileLists.dc.html` — the phone artboard this task drew under D2 — renders the
card's action row as three words (`Refresh · Edit · Remove`, `.rowacts span`).
The shipped card renders three glyphs in the same three full-width 44 px slots.
**Structure, placement and count match; only the label form differs.**

`Lists.dc.html` draws no per-row actions at all (C4 — "artboard silent, not
contradicted"), so the desktop table has no conflict.

The artboard was **left alone**, per the owner's instruction not to touch the
artboards unless a real visual discrepancy requires it. This one is real but
cosmetic and one line of the artboard would settle it either way; it is recorded
here rather than decided.

### Gates

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | **350 passed / 28 files** (was 347 — three new cases, four rewritten) |
| `npm run build` | **60,327 B gzip against 153,600 B — 39.3 %**, brotli 53,957 B. Was 60,082 B; **+245 B**, of which the three sprite symbols are most |
| postbuild assertions | pass |
| Rust | not re-run. No `crates/` or `Cargo.*` path was touched |

### Tests changed

| File | What |
| ---- | ---- |
| `pages/lists/list-row.test.tsx` | the two action cases now read the **accessible name**, not `textContent` — a `textContent` assertion passes on an empty button, which is the failure mode icon actions have. Adds: every action carries label, title and glyph; the `rejected` glyph is `restore` and explicitly **not** the refresh arrow; the pending button is `is-busy`, disabled, and says `Refresh requested for <id>` |
| `styles/grid-tracks.test.ts` | the actions track is now asserted **equal to 3 × 44**, not merely above the widest label; the floor case pins 890 and asserts it fits the 1200 px breakpoint's card; a new case pins Rules as the largest `fr` weight, with the reason |

### The artboard discrepancy — closed

Approved by the owner after it was declared. `MobileLists.dc.html`'s card action
row now draws the same three glyphs the code renders, in the same three
full-width 44 px slots.

| | Before | After |
| - | ------ | ----- |
| ordinary card | `Refresh · Edit · Remove` | refresh · edit · trash glyphs |
| `rejected` card | `Delete and re-add · Edit · Remove` | restore · edit · trash glyphs |

The glyph paths are the sprite's own, inlined because an artboard is a
standalone file with no sprite to reference. Each `<span>` keeps the word as
`title` and `aria-label`, so the artboard records the accessible name the
implementation carries rather than losing it with the label.

**Nothing else about the artboard moved.** The row is still `display: flex` over
a `1px solid #eef2f6` top border, each slot still `flex: 1; min-height: 44px`,
and the card's height is unchanged — so `canvas.json`'s
`{ w: 390, h: 1760 }` entry needs no edit and did not get one. The only CSS
added is the 17 px glyph sizing; the only CSS removed is the 12.5 px label
`font-size`, which no longer has a label to size.

Verified by rendering the exact fragment in a browser: all six glyphs draw a
non-zero bounding box (17 × 17 and 16 × 16), each in a 122 × 44 slot at the
artboard's 390 px width, and the `rejected` card's `restore` arc is visibly the
mirror of `refresh` rather than the same shape.

`Lists.dc.html` needed no change — it draws no per-row actions at all (C4).

**Sketch fidelity now holds in both directions** on this task's two Lists
artboards: the desktop table has no drawn actions to disagree with, and the
phone card draws what ships.
