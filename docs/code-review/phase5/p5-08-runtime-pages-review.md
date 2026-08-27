# p5-08 — Runtime Pages · Review

**Task:** [p5-08-runtime-pages.md](../../../plan/wip/phase5/p5-08-runtime-pages.md) ·
**Plan:** [p5-08-runtime-pages-plan.md](../../../plan/wip/phase5/p5-08-runtime-pages-plan.md) ·
**Branch:** `phase5-08` · **Depends on:** `p5-07`

---

## Implementation Summary

### What was implemented

Three shipped routes — `/cache`, `/performance`, `/upstreams` — all `built: true`,
all `ownsHeader: true`, all declaring `events: []`. The task also lands the
second (and last) chart family in the phase: `charts/lines.ts` for multi-series
lines and filled areas, with `null` gaps and a dashed budget marker drawn by a
hook rather than by a data series.

Work followed the plan's units **U1 → U8**, each ending with `npm run
typecheck`, `npm run test` and `npm run build` green before the next.

### Files and modules

| Area | Files |
| ---- | ----- |
| API client (changed) | `src/api/{history,cache,types,index}.ts`, `src/api/resources.test.ts` |
| Derivations | `src/derive.ts` (+ `derive.test.ts`) — §6's `E*` rows beside p5-06's `R*` |
| Formatting | `src/charts/format.ts` (`formatMiB`, `latencyMsLabel`, `millisLabel`, `microsLabel`, `msAxisLabel`, `epochSeconds`), `src/time.ts` (`clockLabel`) |
| Charts (new) | `src/charts/lines.ts` + `lines.test.ts`, `src/charts/scale.ts` |
| Charts (changed) | `src/charts/{stacked-bars,theme}.ts`, `stacked-bars.test.ts` |
| Components (changed) | `src/components/chart.tsx` (`footer` slot), `status-pill.tsx` (`healthy`) |
| Cache | `src/pages/cache.tsx` + `src/pages/cache/{stage,bounds,counters,clean,swr,cleanup}-card.tsx` + `counter-table.tsx` + `cache.test.tsx` |
| Performance | `src/pages/performance.tsx` + `src/pages/performance/{stage-tiles,latency-chart,qps-card,verdicts-card,reading-card,axis-ends}.tsx` + `{budgets,use-perf-history}.ts` + `performance.test.tsx` |
| Upstreams | `src/pages/upstreams.tsx` + `src/pages/upstreams/{degraded-banner,endpoint-row,states-card,no-pie-card}.tsx` + `upstreams.test.tsx` |
| Wiring / styles | `src/router/routes.ts` (+ `routes.test.ts`), `src/styles/components.css`, `src/styles/grid-tracks.test.ts` |
| Inherited fix | `src/styles/components.css` — p5-06 **F5**, the phone Dashboard cache card |

**No Rust source changed** — `git status -- crates/` is empty. No new API route,
no new config key, no new dependency, no `.md` edited but this file.

Three files the plan's unit lists do not name, each because the alternative was
duplication:

| File | Why |
| ---- | --- |
| `src/charts/scale.ts` | `niceMax` / `ySplits` / `withAlpha` were private to `stacked-bars.ts`; `lines.ts` needs all three, and importing them from the bar chart would drag the bars, label and hover plugins into the Performance chunk |
| `src/pages/cache/counter-table.tsx` | four Cache cards draw the same headerless label/figure list; a sibling card importing another card's export couples two cards that share nothing else |
| `src/pages/performance/axis-ends.tsx` | all three Performance charts print the same two window-end captions (E19) |

Two unit conversions were moved into `charts/format.ts` during the verification
pass, after an audit for stray arithmetic in the page trees found them: the
cleanup gauge's µs → ms (`microsLabel`) and the three charts' RFC 3339 → epoch
seconds (`epochSeconds`, previously `Date.parse(…) / 1000` written out in each
of the three cards). Both are formatting rather than derivation, so `format.ts`
is their home under KTD9 — and with them moved, **the only arithmetic left in
any of the three page trees is `share * 100`** in the failure-run histogram,
turning E22's 0..1 into a CSS height, which is the same shape `FrequencyBar`
already uses.

### Important design decisions

**`derive.ts` carries §6's arithmetic and the pages carry none.** Eleven `E*`
functions with their row ids in the comments, beside p5-06's `R*`. The module
header now states the p5-06 review's own correction — that seven `R*` rows are
counted or laid out where they are printed — so the sentence a reviewer is told
to check is true as written.

**Strategy gates the Upstreams health rendering, and the classifier is one pure
function.** `upstreamMode(strategy)` → `adaptive` | `fallback` | `unknown`.
Under `fallback` the state pill and the four health cells are **omitted**, not
zeroed: API.md is explicit those zeros mean no health state exists to report.
`unknown` is `/config` having failed — counters verbatim, subtitle says so, and
the degraded banner gives both readings attributed.

**Latency `0.0` is an absence, never a measurement.** `latencyMs()` maps an
exact `0.0` to `null`; `spanGaps: false` on every series turns that into a break
rather than a bridge, and a tile whose latest sample is `0.0` prints `—` with
"No traffic in this stage in the latest sample." A real reading is a bucket
upper bound and can never be exactly zero, so nothing is lost.

**The budget is a hook, not a series.** A data series would join the legend,
enter the y range as data, and read as enforced. It is one `hooks.draw` closure
drawing a dashed rule and its caption; the y range is `niceMax(max(data,
budget))` so a stage comfortably inside its target cannot push the marker off
the top. The tile bar takes the neutral accent for the whole span under the
budget and the blocked tone at **≥ 100 %** — the one documented boundary.

**The clean mutation is a plain request.** No confirm dialog, no busy modal, no
navigation block: it is a millisecond-scale cache operation, and the checkbox is
the deliberate choice R2 asks for. It is **not** aborted on unmount — the write
must land — and a response arriving after unmount **skips** `invalidate('cache')`,
because `registry.invalidate` fetches unconditionally and would put a `/cache`
read on the log that no active route owns.

**Performance holds one `/config` reader for both its uses.** The mount snapshot
and the empty-response disambiguation go through the same `readConfig`, which
returns the in-flight promise when one exists — p5-06's **F11**, inherited here.
Measured: an empty answer landing while the mount read is still in flight costs
**one** `/config`, not two.

**`Chart` gained a `footer` slot.** The Performance artboards draw no x ticks:
they name the window's two ends beneath the axis instead, because a per-sample
series over a month has no tick density that is both honest and readable. Those
captions belong to the plot, so they render between it and the footnotes.

### Deviations from the artboards

The plan's registry **X1–X9 is implemented as written**. Five further
departures, all declared here:

| # | Artboard | Shipped | Why |
| - | -------- | ------- | --- |
| new | `Performance.dc.html` draws the x-axis right-hand caption as `now` | the age of the **latest served row** (`13 s ago`, `1 m ago`) | X9's rule applied to the axis: decimation drops whole rows, so the last point plotted is not now. E19 authorises the label as a formatting of `items[].ts`, which is what this is |
| new | `Upstreams.dc.html` tints `failures` red on one row and leaves it plain on two others, with the same treatment for `consecutive` | one rule: `consecutive` takes the good tone at 0 and the bad tone above it; every other counter is neutral | The artboard contradicts itself across its own three rows. `consecutive` is the figure the task singles out as the live one, so it is the one that carries a tone |
| new | `Cache.dc.html` draws the Clean card and the SWR/cleanup column at similar heights | the row is `align-items: start` | Stretched to match, the Clean card is mostly empty space until its first result lands |
| new | p5-06 F5's fix as "pass the segment conditionally" (the plan's preference) | a `<768 px` selector | The application has no viewport listener anywhere; a conditional prop would add the first one to drop one band from one card. `StageBar` lays the bar out with `flex` weights, so hiding the fourth child lets the other three fill the track — which is what the phone artboard draws |
| new | `.pill`'s uppercase would render the chip `BUDGET < 1 MS` | `text-transform: none` on that chip only | The artboard writes the unit `ms` |

### Tests — what they demonstrate

`npm run test` — **639 vitest cases in 45 files** (p5-07 shipped 500 in 40).

| Capability | Demonstrated by | Not covered by tests |
| ---------- | --------------- | -------------------- |
| The perf query is exactly KTD3's five fields, no `to`, no `max_points` | `api/resources.test.ts`, pinned as a list rather than typed | — |
| `cleanCache` omits `?stale` by default | `api/resources.test.ts` | — |
| Every §6 arithmetic row is one function | `derive.test.ts`: E1, E3, E4, E5 (three branches), E10/E12 (`0.0` → gap), E11 (under / at / over budget), E14, E16 (restart-boundary floor), E22 (normalised and all-zero), KTD5 (three modes) | — |
| A `dash` spec is dashed and lighter; an `area` spec fills; every series has `spanGaps: false` | `charts/lines.test.ts` | Canvas drawing — jsdom has no 2D context; read off a browser instead (V10) |
| A budget is one draw hook and **no** extra series, and stays inside the y range | `charts/lines.test.ts` | — |
| The clean flow end to end | `pages/cache.test.tsx`: default-off, `?stale=true` only when chosen, toggling sends nothing, every response field rendered, `invalidate('cache')` exactly once, **zero** invalidate after unmount, the envelope's message on refusal | — |
| `last_duration_micros` renders as one value and no series | `pages/cache.test.tsx` asserts no `.chart` and no `svg` in that card | — |
| E5's three callout branches, and no NaN at zero counters | `pages/cache.test.tsx` | — |
| Performance issues one `/config` + one perf read on entry, one more per range change, none on re-render | `pages/performance.test.tsx` with a request-counting fetch stub | — |
| The two empty answers are distinguishable, and the F11 join holds | `pages/performance.test.tsx`: disabled → full-page state, no chips, no charts; empty range → per-card states after **one** re-read; an empty answer beating the mount read home costs **one** `/config` | — |
| All three failure paths | `pages/performance.test.tsx`: failed perf read → `ErrorState` with chips live; failed mount `/config` → charts draw, 60 s subtitle; failed re-read → per-card empty states, loading released | — |
| KTD7 on a tile, and the latest **served** row | `pages/performance.test.tsx` | — |
| No average anywhere on the page | `pages/performance.test.tsx` asserts `/\bmean\b/` is absent and the two "never an average" statements are present | — |
| `fallback` omits the pill and the four health cells; `adaptive` renders all eight | `pages/upstreams.test.tsx` | — |
| `family: null` → `family unknown`, never `null`; the footer sentence appears only when a row is unknown | `pages/upstreams.test.tsx` (component and page) | — |
| `penalty_round` renders only while penalized | `pages/upstreams.test.tsx` | — |
| E22 all-zero draws four **empty tracks**, and no bar is ever recoloured | `pages/upstreams.test.tsx` | — |
| The banner's three strategy branches, and that it carries no controls | `pages/upstreams.test.tsx` | — |
| `/config` failure → `strategy unknown` subtitle with counters still rendered | `pages/upstreams.test.tsx` (page-level) | — |
| The route declarations, including C1 | `router/routes.test.ts`: `/cache` is `['cache','telemetry']`, the three `built: true` flips, `REFRESH_ENDPOINTS` still exactly five | — |
| The new grids fold 4 → 2, the endpoint row 3 → 2 → 1 zone, the tints are theme-paired token pairs, the clean choice is a 44 px row | `styles/grid-tracks.test.ts` | jsdom resolves no layout — widths measured in a browser (V12) |
| p5-06 F5 closed, and **not** applied to the Cache page's own bar | `styles/grid-tracks.test.ts` | — |

### Measurements

Chromium via Playwright against `fastadhunter:p5-05` in Docker on the dev box
(`fah-p506`, published on 18443), the Vite dev server proxying to it. Dev-box
figures for a bundle and a browser; nothing here is an RB5009 measurement.

**Bundle — gzip gates, brotli is what travels** (assets over 1 kB raw; full list
in the build output):

| file | raw | gzip | brotli |
| ---- | ---: | ---: | ---: |
| `assets/uPlot.esm-*.js` | 50,996 | 21,997 | 19,884 |
| `assets/style-*.css` | 44,781 | 9,323 | 8,299 |
| `assets/index-*.js` | 27,091 | 8,767 | 7,816 |
| `assets/dashboard-*.js` | 18,062 | 6,518 | 5,857 |
| `assets/lists-*.js` | 14,970 | 4,783 | 4,199 |
| `assets/policies-*.js` | 14,773 | 4,866 | 4,310 |
| **`assets/performance-*.js`** | **11,154** | **4,114** | **3,624** |
| `assets/clients-*.js` | 10,860 | 3,893 | 3,419 |
| `assets/jsxRuntime.module-*.js` | 10,735 | 4,496 | 4,114 |
| `assets/rule-tester-*.js` | 10,079 | 3,513 | 3,072 |
| **`assets/cache-*.js`** | **8,259** | **3,025** | **2,631** |
| **`assets/upstreams-*.js`** | **7,912** | **2,742** | **2,380** |
| `assets/rules-*.js` | 7,418 | 2,981 | 2,580 |
| `assets/sprite-*.svg` | 5,501 | 1,185 | 1,034 |
| `assets/assignment-*.js` | 5,278 | 2,058 | 1,846 |
| `assets/icon-*.js` | 4,448 | 2,048 | 1,838 |
| `assets/ranges-*.js` | 3,567 | 1,705 | 1,512 |
| `assets/validation-*.js` | 2,323 | 1,108 | 956 |
| `assets/login-*.js` | 1,939 | 981 | 840 |
| `assets/derive-*.js` | 1,381 | 649 | 587 |
| `assets/empty-state-*.js` | 1,332 | 643 | 602 |
| `assets/refresh-cluster-*.js` | 1,239 | 702 | 609 |
| `index.html` | 1,130 | 611 | 426 |
| *(13 assets under 1 kB raw)* | 3,946 | 3,169 | 2,786 |
| **TOTAL** | **271,020** | **96,408** | **85,674** |

**96,408 B gzip against the 153,600 B budget — 62.8 %.** Brotli 85,674 B.
p5-07 shipped 82,407 B gzip; three pages plus the second chart family added
**14,000 B gzip**, against the plan's 8–12 kB estimate. Of that, ~4.6 kB is the
three page chunks' own JS, ~1.7 kB the shared `ranges` chunk, and **~1.0 kB the
stylesheet** (8,293 → 9,323).

**Chart construction, read off the browser:** `fahChartBuilds()` moved from 0 to
**3** on entering `/performance` (one per card) and stayed at **3** across a
range change — the plot takes new data through `setData`, it is not rebuilt
(p5-05's finding m4). Over five navigation rounds it read 18, i.e. exactly 3 per
mount with none retained.

### Verification

Read off a live browser, a request log, and the API container's own network
namespace. Method notes are given where a check could not be produced.

| # | Result |
| - | ------ |
| **V1** | **PASS.** Entering `/cache` issued **zero** one-shots — the only API reads were `/api/v1/cache` and `/api/v1/telemetry`, plus the shell's own boot `/health`. `activeTimers()` read **2**; union `[]`; socket `closed`. |
| **V2** | **PASS.** Entering `/performance` issued exactly `GET /api/v1/config` **+ one** `GET /api/v1/history/perf?from=…&fields=qps,queries_delta,blocked_delta,allowed_delta,latency` — KTD3's set exactly, no `max_points`, nothing memory-shaped. A range change issued **exactly one** more perf request and **zero** `/config`. `activeTimers()` read **0** throughout. Parked: see V2a. |
| **V2a** | **PASS.** Parked on `/performance` for **305 s** with `activeTimers()` **0** and the socket `closed`: **zero** API requests, counted by a `PerformanceObserver` accumulating into `sessionStorage` so the figure survives a dev-server reload (the observer recorded 0 reloads over the window). |
| **V3** | **PASS.** Entering `/upstreams` issued exactly one `GET /api/v1/config`; `/telemetry` and `/health` came through the shared refresh; `activeTimers()` read **2**; union `[]`; socket `closed`. |
| **V4** | **PASS — server-side.** Counted inside the API container's own network namespace (`docker run --rm --network container:fah-p506 busybox netstat -tn`), the p5-07 V2a method. Parked on each in turn: `/cache` **0**, `/performance` **0**, `/upstreams` **0** ESTABLISHED to `:8443`. Parked on the Dashboard for contrast: **1**. Union `[]` and socket `closed` on all three. **The indicator reads `live`, not `not needed here`** — `shell.tsx:87-94` substitutes the API's reachability on any route whose own state is `not-needed-here`, which is `p5-05`/`p5-06` behaviour this task does not change. The plan's V4 wording predates that substitution. |
| **V5** | **PASS.** On `/cache`, with `visibilityState` forced to `hidden` and `visibilitychange` dispatched: `activeTimers()` **2 → 0**, socket `closed`, and **zero** API requests over the following 8 s. Returning to visible restored `activeTimers()` to **2**. No revalidation request fired on return, which is `isStale()` as documented — the reading was ~10 s old against a 300 s interval. Performance issues nothing on visibility alone (it holds nothing to stop). |
| **V6** | **PASS.** `POST /api/v1/cache/clean` observed **without** `stale` by default and as `?stale=true` only after the checkbox was ticked; ticking it alone sent **zero** requests. The panel matched the response field for field (`stale removed 67`, `entries before → after 67 → 0`, `freed 0 MiB`, `took 0.01 ms`), the RSS note rendered, `/api/v1/cache` was re-read **once**, and the stage bar moved to `0 of 10,000` at once. The checkbox kept its state after success, as §9 specifies. |
| **V7** | **PASS — staged live.** `POST /api/v1/config {"history":{"enabled":false}}` answered `{"applied":true,"restart_required":false}`. `/performance` then rendered the full-page **"History is not being recorded"** state with **0** charts, **0** chips and **0** tiles, and the "Reading this page" card intact — and **one** `/config` only, the disambiguation being short-circuited by the snapshot. Re-enabled and confirmed `history.enabled: true`. The empty-range half was then staged in the browser (below). |
| **V7a** | **PASS — browser-staged.** With recording genuinely on and `/history/perf` stubbed to `items: []` in the page, a range change rendered **three per-card "No data in this range"**, kept the chips live, showed no full-page state, and cost **exactly one** disambiguating `/config`. Visibly distinct from V7 by construction and in the DOM. |
| **V8** | **PASS — staged live, no restart needed.** The dev container is already running `[dns.upstreams] strategy = "fallback"` (confirmed from `GET /config`). `/upstreams` rendered: subtitle `strategy fallback`, **zero** state pills, **four** counter cells per row (attempts / failures / consecutive / TLS handshakes), the four health cells absent, the histogram present, and the legend card swapped for the fallback explanation. Zeros are nowhere presented as health. |
| **V9** | **PASS — browser-staged, not API-staged.** `/health` was stubbed to `degraded` in the page and the health cluster's Refresh pressed. The banner rendered `banner warn` (`--pill-warn-bg`, `--pill-warn-fg` border), never `bad`, with the **fallback-correct** sentence — because the container really is on `fallback` — ending "Clients are being served.", and carrying **no** controls. **The API was not made to report `degraded`:** doing so needs unroutable upstreams in `[dns.upstreams]`, which is boot-only and would mean editing and restarting the owner's dev fixture twice. The three banner branches are unit-tested. |
| **V10** | **PASS — both tones observed on real data.** Only per-stage percentile series and tiles exist; `/\bmean\b/` is absent from the page text and both "never an average" statements are present. The budget renders as a dashed rule with its `1.0 ms — budget` caption inside the plot. Under budget, every tile bar was the neutral accent (`.stage-bar-fill.over` count **0**). Then real traffic produced a genuinely over-budget sample — `forward_p99 = 0.1 s`, the top finite bucket — and that tile alone flipped to the blocked tone at `100.000 ms` while `block` and `cache-hit` stayed neutral at `0.100 ms`. The chip reads `BUDGET < 1 ms` in all three, never `< 1 MS`. |
| **V11** | **NOT RUN.** `stride > 1` is unreachable on this fixture: a 30 d request returned **708 items at `stride: 1`**, against the server's 1000-point perf default. The footnote is wired to the response's `stride` through the p5-05 `Chart` wrapper and is asserted by `pages/performance.test.tsx` with a stubbed `stride: 44` (rendered "one point every 44 buckets"). A live row needs > 1000 samples, i.e. > ~17 h of recording at 60 s. |
| **V12** | **PASS.** Both themes at **1400 / 1200 / 900 / 390 CSS px** on all three routes: `scrollWidth − clientWidth` is **0** on both `documentElement` and `.main`, 24 measurements, no exceptions. At 390 px the only control measuring under 44 px is the clean checkbox's own 18 × 18 box, whose target is a 44 × 44 `::before` — the `.switch` precedent exactly. Two real gaps were found by this measurement and fixed: the **header-placement refresh cluster** arrived 55 × 25 / 26 × 24 at 390 px (p5-06 sized `.ctl.in-card` only, because every cluster it shipped was in a card), and the Endpoints card's configured-order note was squeezed into a four-line column beside the cluster. |
| **V12a** | **PASS.** §10's mobile shapes at 390 px: Upstreams is one card per endpoint with the counter grid at two columns and the histogram full width beneath; Cache is single-column with both bars full width and the stage legend at two columns; Performance stacks its tiles and puts the range chips in a full-height row above the plot. |
| **V13** | **PASS.** Every rendered figure traces to a field or to a §6 `E` row — the table below. No derivation exists that §6 does not list. `allow` is labelled `allow` and `permitted` appears on none of the three pages. Re-checked against populated pages, not empty ones: the fixture was driven with ~3,300 real DNS queries and every card then carried live figures (Cache 589 entries / 61.8 % hit rate / SWR 20 enqueued; Upstreams 1,435 and 4 attempts with `consecutive 4` on the secondary and a one-bucket histogram; Performance 19.8 qps latest, 21.5 busiest). |
| **V14** | **PASS.** Figures above. Three new lazy route chunks. **No chunk from this task references `uplot`** — asserted by scanning every built asset: the only three hits are `uPlot.esm-*.js` itself, the **dynamic** `import()` in the shared `ranges-*.js` chunk (`charts/runtime.ts`, which is KTD2's contract), and p5-06's hand-written `.chart .uplot` CSS scope. The postbuild assertions pass. |
| **V15** | **PASS in part.** Five navigation rounds across the three pages: `activeTimers()` **2** on Upstreams, union `[]`, socket `closed`, chart constructions **18** (3 per Performance mount, none retained), and the clean result panel does not survive a round. **The heap half was NOT produced:** `usedJSHeapSize` read 11.7 → 17.5 MiB across the five rounds with no collection observed, and without a forced GC (`--expose-gc` / CDP `HeapProfiler.collectGarbage`) that is evidence in neither direction — the same limitation p5-07's V19 records. |
| **V16** | **PASS.** All six gates green — see below. `routes.test.ts` pins C1 and the three `built: true` flips. |

### Every rendered figure, and where it comes from (V13)

**Cache**

| Display | Source |
| ------- | ------ |
| `fresh` / `stale` / `expired` counts and bands | `/cache` fields, verbatim |
| `free` band and count | **E1** — `max(0, capacity − entries)` |
| `1,108 of 50,000` | **E2** — formatting of `entries`, `capacity` |
| `entries 0 / 10,000` · `0.0 %` | `entries`, `capacity`, `load_percent`, verbatim |
| `bytes 1.1 MiB / 64 MiB` · `1.7 %` | **E6** formatting of `bytes` / `max_bytes`; `byte_load_percent` verbatim |
| "X is the bound closest to evicting" | **E5** — comparison only; three branches |
| `lookups` | **E3** — `hits + misses` |
| `hits` / `misses` / `evictions` | verbatim |
| `88.5%` ring | **E4** — `hits / (hits + misses) × 100`, 0 at zero denominator |
| `(0 expired entries right now — this would remove 67 stale ones)` | **E7** — formatting of `expired`, `stale` |
| `Last clean, 22:45` | **E8** — client receive time, session-local |
| clean result rows | `CacheCleanResponse` verbatim; `freed` via **E6**, `took` via `millisLabel` |
| SWR five counters | `telemetry.counters.swr`, verbatim |
| cleanup `runs` / `entries removed` | verbatim |
| `bytes freed 9.4 MiB` | **E6** |
| `last run took 1.84 ms` | **E9** — `last_duration_micros / 1000`, a current value only |

**Performance**

| Display | Source |
| ------- | ------ |
| tile figures / `—` | **E10** — latest served `latency.<stage>_p99 × 1000`; exact `0.0` → em-dash |
| tile bars and their tone | **E11** — `value_ms / 1.0`, capped; tone flips at ≥ 100 % |
| latency series | **E12** — `*_p50` / `*_p99 × 1000`, exact `0.0` → `null` gap |
| `1.0 ms — budget` | **E13** — `budgets.ts` constant, PERFORMANCE.md §Budgets |
| `latest sample` / `busiest served sample` | **E14** — over the served rows only |
| `20 k+` | **E15** — `budgets.ts` constant, labelled measured on the RB5009 |
| `pass` band | **E16** — `queries_delta − blocked_delta − allowed_delta`, floored at 0 |
| `block` / `allow` bands | `blocked_delta` / `allowed_delta`, verbatim |
| qps series | `qps` per item, verbatim |
| decimation footnote | **E17** — `stride` through `Chart`'s own footnote |
| `one row per 60 s` | **E18** — `history.sample_interval_seconds`, 60 when absent |
| `11 h ago … 13 s ago` | **E19** — formatting of `items[].ts` |

**Upstreams**

| Display | Source |
| ------- | ------ |
| index badge | **E20** — array index, the answering-endpoint identity |
| `address` / `protocol` / `state` | verbatim |
| attempts / failures / consecutive / TLS handshakes / penalties / probes / probe successes | verbatim |
| `penalized for 312 s` | **E21** — formatting with the unit; no duration arithmetic |
| histogram bar heights | **E22** — normalised to the row's own max; all-zero → empty tracks |
| `2 · 1 · 0 · 0` | `failure_runs`, verbatim |
| `round N` | **E23** — `penalty_round`, only while `state === 'penalized'` |
| `family unknown` | **E24** — `family === null`; footer sentence when any row is unknown |
| banner text | **E25** — branch on `health.status` × strategy |
| `strategy fallback` | `dns.upstreams.strategy`, verbatim |

Nothing else is derived. Absent by design on all three pages: any latency
average, any upstream share of traffic / success rate / availability / health
score, anything computed from `tls_handshakes`, any delta of two telemetry
reads, any delta of `last_duration_micros`, any per-query upstream attribution,
any figure combining `/stats` with these endpoints.

### Gates

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | **1,196 passed, 0 failed** — unchanged from the post-p5-07 tree, as expected for a task that touched no Rust |
| `npm run typecheck` | clean |
| `npm run test` | **639 passed** in 45 files |
| `npm run build` | clean, **62.8 %** of the 153,600 B gzip budget (96,408 B gzip, 85,674 B brotli) |

The browser console is clean on all three routes: **0 errors, 0 warnings** after
a reload on the final tree.

### Known limitations and deferred items

1. **V11 NOT RUN** — `stride > 1` is unreachable against a fixture holding 708
   perf rows. Covered by a unit test with a stubbed stride.
2. **V9 is browser-staged, not API-staged.** Making `/health` report `degraded`
   needs unroutable upstreams in the boot-only `[dns.upstreams]` block and two
   restarts of the owner's dev fixture. The three banner branches are
   unit-tested; the live check rendered the real page against a stubbed
   `/health` body.
3. **V15's heap half was not produced** — no forced GC available.
4. **The indicator reads `live` on all three routes**, not `not needed here`.
   That is `shell.tsx`'s deliberate substitution for routes whose own state is
   `not-needed-here`, shipped in p5-05/p5-06 and untouched here. The plan's V4
   wording predates it. Recorded rather than changed.
5. **`0 MiB` is a real rendering.** `formatMiB` rounds to one decimal, so a
   cache holding under ~52 kB of estimated entry heap prints `0 MiB`. Honest at
   that precision; noted because it looks like a missing figure.
6. **p5-06 F12 is still open** — `/health` is fetched twice at boot. Observed
   again here; it is `shell.tsx`'s, deferred to whoever revisits it.
7. **The dev fixture was left as found**, with two exceptions: the DNS cache was
   emptied by V6's stale purge (it refills from traffic), and `history.enabled`
   was toggled off and back on for V7 (confirmed `true` afterwards). Real DNS
   traffic was then driven at the container to repopulate the cache and the
   upstream counters — no fixture data was written and no response was faked.
8. **Two `@media` blocks in `components.css` now touch p5-05/p5-06 selectors**
   (`.ctl.in-card`, `.hd .ctl`, `.z-cache .seg`). Each is commented with what it
   fixes and why the rule sits where it does.

### Documentation

**No repository document was changed but this file.** The plan's §13 proposes
three edits, none applied, all awaiting the owner's yes:

| Document | Proposed change | Reason |
| -------- | --------------- | ------ |
| `docs/dashboard/information-architecture.md` §Performance | drop or re-scope the "RSS and peak RSS · cache entries and hit ratio" chart list | C2 — the doc otherwise contradicts the shipped page, which draws latency, QPS and verdict deltas only |
| `docs/code-review/phase5/p5-06-dashboard-and-lists-review.md` | mark deferred **F5** closed by p5-08 U7 | the review said "worth doing beside p5-08's Cache page" |
| `plan/wip/phase5/CLAUDE.md` | status flip for task 8 | workflow bookkeeping, owner-performed |

Five artboard deviations beyond the plan's X1–X9 are declared above and belong
in the same decision.

---

## Findings

Reviewed 2026-08-27 against `plan/wip/phase5/p5-08-runtime-pages-plan.md`
(R1–R13, C1–C14, X1–X9, KTD1–KTD10, §6 E1–E25) and the Rust the types are
written against (`fah-api/src/wire.rs`, `fah-api/src/routes.rs`,
`fah-model/src/perf.rs`, `fah-dns/src/upstream/mod.rs`).

### What was re-verified, and how

| Claim | Method | Result |
| ----- | ------ | ------ |
| Gates | re-ran `npm run typecheck`, `npm run test`, `npm run build` | clean; **639 passed in 45 files**; **96,408 B gzip (62.8 %)**, 85,674 B brotli — total identical to the recorded figure, three per-asset gzip cells reproduce within ±8 B |
| No Rust touched | `git status -- crates/` | empty; the recorded cargo figures carry over unre-run |
| KTD2 — no static `uplot` | `grep -l uPlot dist/assets/*.js` | two hits only: `uPlot.esm-*.js` and `ranges-*.js` (the dynamic `import()` in `charts/runtime.ts`). Confirmed |
| Wire shapes | `CacheCleanResponse`, `HistoryPerfResponse`, `PerfSampleResponse`, `LatencySummary`, upstream status read against `api/types.ts` | field-for-field match, including `skip_serializing_if` → optional-not-null on `PerfItem` |
| KTD3 | `PERF_FIELDS` pinned as a list; `historyPerfQuery` emits `from` + `fields` only | confirmed in source and in `api/resources.test.ts` |
| §6 completeness | grepped the three page trees for arithmetic | one deviation from the summary's own wording — see N3 |
| E22 all-zero | `shareOfMax(v, 0)` returns 0 (`derive.ts:39-41`) | no division by zero |
| E16 restart floor | `passDelta` floors at 0 | confirmed |
| p5-06 F5 | `StageBar` renders all four segments or none (`stage-bar.tsx:26-34`), so `:nth-child(4)` is always `free` | the CSS-only fix is positionally sound |
| C9 gating | `EndpointRow` sets `health = mode !== 'fallback'`; four cells and the pill omitted | matches C9/C14 |
| KTD8 post-unmount | `cache.test.tsx:324-332` asserts zero `invalidate` | confirmed |
| Shared-selector blast radius | `placement="header"` used only by `/cache` and `/upstreams`; `.ctl.in-card > .note` reachable only from the Upstreams Endpoints cluster (`RefreshCluster` is the only source of a direct-child `.note`) | the two `@media` additions are correctly scoped — but see **F3** for a third, undeclared one |

No Critical and no Major finding. Four Minor, five Nitpick.

---

### F1 · Minor — the stage tiles and the QPS stat row ignore `error`, and print the wrong window

**Where:** `pages/performance.tsx:87`, `pages/performance/stage-tiles.tsx:33-52`,
`pages/performance/qps-card.tsx:60-71`.

`StageTiles` takes only `items`; the QPS stat row sits outside `QpsCard`'s
`body()` and so outside its `if (error !== null)` guard. `usePerfHistory` does
not clear `history` on a failed read (`use-perf-history.ts:96-101`), which is the
right call for the chart — the previous range stays plotted. It is the wrong
call for these two, which are unguarded:

- **Mount failure.** `history` is `null`, so every tile renders an em-dash under
  "No sample in the selected range." The read failed; whether the range holds
  samples is unknown. The card beside the tiles says so correctly through
  `ErrorState`; the tiles state the opposite.
- **Range-change failure.** `history` still holds the *previous* range. Three
  tiles and two stat figures then render that range's numbers under labels that
  say "the selected range" and "latest **served** sample", beside three charts
  showing an error. Nothing on the page marks them stale.

**Impact:** a figure attributed to a window it did not come from, which is the
one thing R1 and the §6 provenance table exist to prevent. Reachable on any
`/history/perf` 5xx after a successful first load — a path the page's own
`ErrorState` branch already treats as expected.

**Evidence:** source; `performance.test.tsx:238-255` asserts the error message
and the live chips but makes no assertion about the tiles, so the suite does not
see it.

**Recommendation:** fix before `DONE` — pass `error` into `StageTiles` and move
the stat row inside `QpsCard`'s guard (or render em-dashes in both while
`error !== null`). Small and local.

---

### F2 · Minor — the budget chip is green, including on a tile that is over budget

**Where:** `pages/performance/stage-tiles.tsx:76` — `class="pill good stage-budget"`.

`BUDGET < 1 ms` is a static label, not a state, but `.pill.good` paints it in
`--pill-good-fg` / `--pill-good-bg` (`components.css:180-183`). When a stage
crosses its budget the proximity bar flips to `--series-blocked`
(`.stage-bar-fill.over`) while the chip directly above it stays green — exactly
the tile where the tone matters most. V10 observed that state live
(`forward_p99 = 0.1 s`), so the pairing has already shipped.

**Impact:** colour asserting "good" against the bar beside it; a reader scanning
tone first reads the wrong verdict. It also cuts against X3's own framing, which
put the one documented boundary in the bar precisely so nothing else would carry
a tone.

**Recommendation:** fix before `DONE` — `.pill.neutral` exists
(`components.css:176-179`) and is the correct class for a label.

---

### F3 · Minor — `.chart-legend` is redefined globally, and the change is not declared

**Where:** `styles/components.css:3052-3055` re-opens `.chart-legend`, first
defined at `:599-605` by p5-06.

```css
.chart-legend {           /* p5-08 · Performance section */
  flex-wrap: wrap;
  align-items: baseline;
}
```

It is unscoped, so it also applies to `pages/dashboard/queries-over-time.tsx:150`
and `pages/dev-gallery.tsx:309`. `align-items: baseline` in particular re-aligns
the `.sw` swatches in the Dashboard legend p5-06 shipped.

**Impact:** low — `flex-wrap` is an improvement and the swatch shift is
cosmetic. The problem is bookkeeping: known-limitation 8 enumerates the shared
selectors this task touches as exactly three (`.ctl.in-card`, `.hd .ctl`,
`.z-cache .seg`), and this is a fourth, outside a media block, unmentioned and
unasserted. A later reader trusting that list will not look here.

**Recommendation:** either scope it (`.latency-legend` and the two verdict/QPS
cards) or add it to limitation 8 and pin it in `grid-tracks.test.ts`. Either is
fine; leaving the list wrong is not.

---

### F4 · Minor — the window-end captions freeze for the life of the visit

**Where:** `pages/performance/axis-ends.tsx:20` — `const now = Date.now()`, read
at render.

This is the one route in the application that holds no timer and no subscription
(KTD1, V2a), so nothing re-renders it while it is parked. The right-hand caption
is `formatAge(items[last].ts, now)`, and V2a parked on this page for **305 s**: a
caption reading `13 s ago` at mount still reads `13 s ago` five minutes later.
The Dashboard's `DataAge` has the same shape but is re-rendered by the refresh
tick this page deliberately does not have.

**Impact:** the caption understates the age of the latest served row without
bound — and it is the one label the deviation registry defends *specifically*
because "the age of the latest served row" is more honest than the artboard's
`now`. A relative age is a live figure, rendered by a page that has decided not
to be live.

**Recommendation:** defer or fix, owner's call. The cheapest honest fix is an
absolute label for both ends (the clock or date of `items[0].ts` and
`items[last].ts`) — still a formatting of the same field, true for ever, no
timer added. Adding a timer to keep a caption fresh would be the wrong trade
against R8.

---

### N1 · Nitpick — `/history/perf` is issued on mount even when the recorder is off

`usePerfHistory`'s effect runs on mount alongside the `/config` read
(`performance.tsx:56-64`, `use-perf-history.ts:83-102`), so on a deployment with
`history.enabled = false` every entry to `/performance` costs one perf request
whose answer the page never renders — V7's own staging shows the full-page state
with that request already spent. Gating the perf effect on `config !== null`
removes it but serialises the two reads and adds a round trip to the normal
path, which is the worse trade. **Recommendation: accept, and state it** — the
V7 row records "one `/config` only" and is silent on the perf read beside it.

### N2 · Nitpick — the endpoint list is keyed on `address`, not on the index it calls the identity

`upstreams.tsx:97` uses `key={upstream.address}` while E20 and `endpoint-row.tsx`'s
own header make the array index the answering-endpoint identity. `[dns.upstreams]`
does not forbid the same address twice, and duplicate keys reconcile wrongly in
Preact. Rows are positional and in configured order, so `key={index}` is both
correct and consistent with what the page says the identity is.

### N3 · Nitpick — the Implementation Summary's "only arithmetic" sentence is not accurate

The summary states that *the only arithmetic left in any of the three page trees
is `share * 100`*. Three others are present, all defensible, none in `derive.ts`
or `format.ts`:

| Where | What |
| ----- | ---- |
| `pages/cache/bounds-card.tsx:80` | `Math.max(0, Math.min(100, percent))` — a display clamp on a served field |
| `pages/performance/qps-card.tsx:63,67` | `.toFixed(1)` inline; KTD9 puts display formatting in `charts/format.ts`, where `latencyMsLabel` and `millisLabel` already live |
| `pages/performance/use-perf-history.ts:15` | `now - RANGES[range].spanMs` — the range window, not a rendered figure |

The claim, not the code, is what wants correcting; `qps-card`'s two are the only
ones that arguably belong in `format.ts` under KTD9.

**Closed in part** by the audit cleanup below: `qps-card`'s two `toFixed(1)`
calls moved to `charts/format.ts`. The clamp and the range window stay where
they are — neither is a rendered figure — so the sentence itself still wants the
correction this row asks for.

### N4 · Nitpick — `failure_runs` is typed `number[]` against a fixed `[u64; 4]`

`api/types.ts:109`. `fah-dns/src/upstream/mod.rs:215` builds exactly four buckets
and `wire.rs` serialises the array as-is, so a four-tuple would make
`endpoint-row.tsx`'s fixed `runs of length 1,2,3,4+` caption and its four
`.run-track`s a type-level fact rather than a convention.

### N5 · Nitpick — the budget caption sits on the plot's top edge whenever nothing is over budget

With every p99 sample under 1 ms, `niceMax(max(data, 1))` returns exactly `1`, so
`budgetPlugin` draws the rule at `u.bbox.top` and its caption at `y − 3 px` with
`textBaseline: 'bottom'` — above the plot area, in whatever top padding uPlot
computed. V10 observed the caption rendering in that state, so this is inference
against a measurement and the measurement wins; recorded only because a future
padding or font change would break it silently. A one-line guard (draw the
caption below the rule when `y` is within the label's height of `bbox.top`) would
make it structural.

### N6 · Nitpick — E10's row selection lived outside `derive.ts`

Found while auditing N3. The plan's U2 lists **eleven** arithmetic E-rows for
`derive.ts`; the module exported **ten** functions. The missing one was E10's
*selection* — which row a tile reads — implemented as a local `latestLatency` in
`pages/performance/stage-tiles.tsx`, while its sibling **E14** (`qpsStats`),
which scopes over the served rows by the same rule, sat in `derive.ts`.

**Impact:** bookkeeping, not behaviour. The module's own header claims "one
module against one table"; a reviewer checking §6 against `derive.ts` would find
ten of eleven rows and no pointer to the eleventh. The asymmetry between the two
halves of one served-rows rule is what made it easy to miss.

**Closed** by the audit cleanup below.

---

### Housekeeping

`docs/code-review/phase2.6/p2.6-11-optin-deploy-soak-review.md` is still modified
in the working tree from the 2.6 track. The plan's inherited-obligations table
calls it out: stage p5-08 paths explicitly, never `git add -A`.

---

### Status

**PASS WITH DEFERRED FINDINGS.**

The task meets R1–R13 as built: the §6 table is complete and one-to-one with
`derive.ts`, the three routes' request discipline holds in source and in the
recorded browser evidence, and every wire type matches the Rust it was written
against. No finding is architectural and none blocks.

- **F1**, **F2**, **F3** — **FIXED**, see below.
- **N6** — **FIXED**, and **N3** closed in part, by the audit cleanup below.
- **F4** is open and is the owner's call — a stated trade either way.
- **N1**, **N2**, **N4**, **N5** are recorded, not requested, and untouched.

---

## Fixes applied — 2026-08-27

F1, F2 and F3 fixed on the owner's approval. F4 and N1–N5 deliberately left
alone; no other behaviour changed.

### F1 — a failed read lends its figures to nothing · FIXED

The retention the plot depends on is unchanged: `usePerfHistory` still keeps the
previous range's rows in state on a failed read, and the chart cards still swap
their body for `ErrorState` while holding them. Only the two surfaces scoped to
the *selected* range stop inheriting them.

| File | Change |
| ---- | ------ |
| `pages/performance/stage-tiles.tsx` | `StageTiles` takes an `error` prop; `latest` resolves to `null` while it is set, and each tile takes `unread` |
| `pages/performance/stage-tiles.tsx` | `StageTile` gains a third idle reason — "The read for this range failed — no figure rather than one from another range." — distinct from "No sample in the selected range." and from the KTD7 no-traffic sentence |
| `pages/performance.tsx:87` | passes `error={perf.error}` |
| `pages/performance/qps-card.tsx:56-62` | the `qpsStats` memo returns `{ latest: null, busiest: null }` while `error !== null`, so latest and busiest render em-dashes. `SUSTAINED_QPS` is a PERFORMANCE.md constant, not range data, and stays |

`ErrorState` remains the only place the API's own message is rendered — the
tiles state that no figure exists, not why.

### F2 — the budget chip is neutral · FIXED

`pages/performance/stage-tiles.tsx:93` — `pill good` → `pill neutral`. The
proximity bar is untouched and remains the only element on the tile carrying a
tone, flipping to `--series-blocked` at ≥ 100 % of budget (X3, E11).

### F3 — the Performance legend rule is scoped · FIXED

`styles/components.css:3056` no longer re-opens `.chart-legend`; the wrapping
rule is now `.latency-legend, .verdicts-legend`. `verdicts-card.tsx:72` gains the
`verdicts-legend` class to be reachable by it. The p5-06 base rule at `:599` is
the single declaration of `.chart-legend` again, so
`pages/dashboard/queries-over-time.tsx` and the dev gallery render exactly as
they did before this task. Known-limitation 8's list of touched shared selectors
is correct as written and needs no amendment.

### Regression tests

Six added; each was confirmed to **fail against the pre-fix source** and pass
after, rather than asserted to be a guard.

| Test | File | Guards |
| ---- | ---- | ------ |
| says the read failed rather than that the range held no sample | `pages/performance.test.tsx` | F1 — mount failure: the new sentence present, the empty-range one absent, three em-dash tiles, stat row `— · — · 20 k+` |
| drops the previous range's tile figures when the new range fails | same | F1 — the 24 h figures `0.039` / `0.051` / `0.412` are absent from the page after a failed 7 d read ✗ pre-fix |
| drops the previous range's QPS stats when the new range fails | same | F1 — `12.5` / `28.4` → em-dashes; the doc constant survives ✗ pre-fix |
| is neutral, so it cannot read as a verdict on the tile | same | F2 — all three chips carry `neutral`, none carries `good` ✗ pre-fix |
| leaves the bar as the only thing on the tile carrying a tone | same | F2 — the over-budget bar still flips |
| scopes its wrapping to this page's legends | `styles/grid-tracks.test.ts` | F3 — `^\.chart-legend \{` occurs exactly once in the sheet ✗ pre-fix; the scoped rule declares `wrap` / `baseline` |

The six existing `StageTiles` unit tests take the new `error={null}` prop; no
existing assertion changed.

### Gates after the fixes

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | **645 passed** in 45 files (639 → 645) |
| `npm run build` | clean, **62.8 %** of the 153,600 B gzip budget |
| bundle | 96,516 B gzip / 85,722 B brotli — **+108 B gzip** on 96,408 (the third idle sentence, the `unread` branch, one CSS selector) |
| cargo | not re-run: `git status -- crates/` is still empty |

---

## Audit cleanup — 2026-08-27

A second, separately approved pass, scoped to N6 and N3's `format.ts` half. Two
mechanical moves, no behaviour change, nothing else touched. F4 and
N1/N2/N4/N5 deliberately left alone.

### N6 — E10's row selection moved into `derive.ts` · FIXED

`latestLatency` moved verbatim from `pages/performance/stage-tiles.tsx` to
`derive.ts`, beside `latencyMs` (E10/E12) and `qpsStats` (E14), and exported.
Its body is unchanged; its doc comment now names it as E10's other half and
states the served-rows scoping it shares with E14.

| File | Change |
| ---- | ------ |
| `src/derive.ts:1` | `import type { PerfItem, PerfLatency } from './api/types'` — the module's first import, type-only, so nothing new reaches a chunk |
| `src/derive.ts:206-222` | `latestLatency` added |
| `src/pages/performance/stage-tiles.tsx:3` | imports it from `derive` instead of declaring it |
| `src/pages/performance/stage-tiles.tsx` | the nine-line local function deleted; `PerfLatency` stays imported for `Stage.key`'s `keyof` |

**All eleven of §6's arithmetic E-rows now resolve to a function in
`derive.ts`** — E1, E3, E4, E5, E10 (value and selection), E11, E12, E14, E16,
E22, E25/KTD5 — which is the property KTD9 and the module header claim.

### N3 (in part) — the two `toFixed(1)` calls moved into `charts/format.ts` · FIXED

Treated strictly as formatting, per KTD9: a one-decimal rendering of a served
field, no arithmetic and no derivation.

| File | Change |
| ---- | ------ |
| `src/charts/format.ts:122-129` | `qpsLabel(qps: number): string` added, beside `latencyMsLabel` and `millisLabel` |
| `src/pages/performance/qps-card.tsx:4` | imports it |
| `src/pages/performance/qps-card.tsx:69,73` | `stats.latest.toFixed(1)` → `qpsLabel(stats.latest)`; same for `busiest` |

The em-dash branches, `SUSTAINED_QPS` and the F1 error guard are untouched.

### Arithmetic sweep after the move

Re-run over the three page trees. The Performance tree now holds **no rendered
arithmetic at all**; the three remaining sites are outside what was approved and
none of them produces a figure the operator reads:

| Site | What | Renders a figure? |
| ---- | ---- | ----------------- |
| `pages/cache/bounds-card.tsx:80` | `Math.max(0, Math.min(100, percent))` | no — a CSS width clamp; the printed percentage is `percent1(percent)`, the served field |
| `pages/performance/axis-ends.tsx:17` | `items[items.length - 1]` | no — a last-element index |
| `pages/performance/use-perf-history.ts:15` | `now - RANGES[range].spanMs` | no — the request window |
| `pages/upstreams/endpoint-row.tsx:109` | `share * 100` | no — E22's 0..1 into a CSS height, `failureRunShares` having done the normalisation |

### Regression tests

Five added to `derive.test.ts`, which is already the home of both modules' tests.

| Test | Guards |
| ---- | ------ |
| takes the last served row, not the first | E10 reads the tail, not the head |
| skips a row whose `latency` key was trimmed away | absent under `fields` is not a measurement of zero |
| answers null when no served row carries one | `[]` and an all-trimmed list |
| prints the stat row's one decimal | `qpsLabel(12.5)`, `qpsLabel(28.4)` |
| keeps the decimal on a whole figure | `20 → 20.0`, `0 → 0.0` — the artboard's form |

The six `StageTiles` component tests and the three F1 page tests already cover
the same selection through the rendered tile and were left as they are; they
pass unchanged, which is what makes this a move rather than a rewrite.

### Behaviour

Unchanged. No rendered string, request, class, branch or timing differs: the two
moved bodies are byte-identical to what they replaced, and every pre-existing
assertion passes without edit.

### Gates after the cleanup

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | **650 passed** in 45 files (645 → 650) |
| `npm run build` | clean, **62.8 %** of the 153,600 B gzip budget |
| bundle | 96,525 B gzip / 85,743 B brotli — **+9 B gzip** on 96,516 |
| cargo | not re-run: `git status -- crates/` is still empty |

Run after the final source state; no source has changed since.

### Status after the cleanup

**PASS WITH DEFERRED FINDINGS**, unchanged in kind. Open and recorded: **F4**
(the owner's call) and **N1**, **N2**, **N4**, **N5** (recorded, not requested).
**N3** keeps only its documentation half — the Implementation Summary's "only
arithmetic" sentence is still the thing that wants correcting, and correcting it
is a `.md` edit awaiting the owner's yes.
