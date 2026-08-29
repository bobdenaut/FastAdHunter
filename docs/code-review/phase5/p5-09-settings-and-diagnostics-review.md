# p5-09 — Settings and Diagnostics · Review

**Task:** [p5-09-settings-and-diagnostics.md](../../../plan/wip/phase5/p5-09-settings-and-diagnostics.md) ·
**Plan:** [p5-09-settings-and-diagnostics-plan.md](../../../plan/wip/phase5/p5-09-settings-and-diagnostics-plan.md) ·
**Branch:** `phase5-09` · **Depends on:** `p5-08`

**Verdict: PASS.** Every blocker and every real defect is closed. Open: **M2**,
**N5**, **N6**, **N8**, **N9**, **N12**, **N16**, **N21**–**N24** and
**T2**–**T4**, all recorded below as non-blocking. §9 closed ten of the
non-blocking rows.

Consolidated after the final pass: superseded measurement blocks and
pre-redesign prose were removed. **No finding was dropped.** One set of numbers,
in §6.

---

## 1 · What shipped

The System section: `/settings`, `/diagnostics/health`, `/diagnostics/memory`
and `/diagnostics/live-feed` — the last four routes of the thirteen. All
`built: true`, all `ownsHeader: true`. `pages/system.tsx`, the not-yet-built
placeholder, is deleted with its last consumer; **no route is unbuilt any more**.

**No Rust source changed** — `git status -- crates/` is empty. No new API route,
no new config key, no new dependency.

### 1.1 · Files and modules

| Area | Files |
| ---- | ----- |
| API client (changed) | `src/api/{types,config,auth,history,index}.ts`, `src/api/resources.test.ts` |
| API client (new) | `src/api/debug.ts` |
| Shared infrastructure | `src/refresh/registry.ts` (+ `registry.test.ts`, `observe-callers.test.ts`), `src/system/restart-banner.ts` (+ test), `src/shell/restart-banner.tsx`, `src/shell/shell.tsx`, `src/services.ts` |
| Moved (KTD4) | `pages/upstreams/degraded-banner.tsx` → `components/degraded-banner.tsx` |
| Components (changed) | `src/charts/format.ts` (`formatBytes`), `src/lifecycle/timers.ts` (`onNextFrame`). `components/donut.tsx` was extended here and later **reverted to HEAD** — the Memory redesign removed its only caller |
| Derivations | `src/derive.ts` (+ `derive.test.ts`) — D3–D5, D7, D10–D14 and KTD7 beside p5-06's `R*` and p5-08's `E*` |
| Settings | `src/pages/settings.tsx` + `src/pages/settings/{metadata,patch,field-row,server-list,section-card,raw-panel,access-card,password-dialog,rotate-dialog}` + `settings.test.tsx` + `settings/patch.test.ts` |
| Health | `src/pages/diagnostics-health.tsx` + `src/pages/health/{endpoint-summary,outcomes-card,backpressure-card,rule-lists-card,engine-card,never-card}.tsx` + `diagnostics-health.test.tsx` |
| Memory | `pages/diagnostics-memory.tsx` + `pages/memory/{kpi-rail,sparkline,key-metrics-card,trend-options,composition-card,trend-card,faults-card,allocator-card,budgets}` + `diagnostics-memory.test.tsx`. `rail.tsx` and `leak-watch-card.tsx` deleted |
| Live Feed | `src/pages/live-feed.tsx` + `src/pages/live-feed/{ring,filters,detail}` + `live-feed.test.tsx` |
| Shell (Memory rebuild) | `shell/{topbar,shell,content-header}.tsx`, `assets/sprite.svg` (+`memory` symbol), `charts/theme.ts` |
| Wiring / styles | `src/router/routes.ts` (+ `routes.test.ts`), `src/styles/{components.css,tokens.css}`, `src/styles/grid-tracks.test.ts`, `src/pages/system-invariants.test.ts`, `src/pages/dev-gallery.tsx` (+ test) |
| Deleted | `src/pages/system.tsx` |

### 1.2 · Design decisions

**The form's schema is a module, not a response.** `metadata.ts` holds 13
sections × 29 fields as data: dotted key, control kind, bounds, enum values,
mutability class, help line and source anchor. `patch.ts` is the pure write path
over it. `GET /api/v1/config` is read for **values only**. Anchors:

| Anchor | What it supplied |
| ------ | ---------------- |
| CONFIGURATION.md §Reference | field grouping, section order, help text, defaults |
| CONFIGURATION.md §Mutability classes | the live/restart split |
| `fah-api/src/config_store.rs` `BOOT_KEYS` | which keys are boot — the authority, since it is what the endpoint answers from |
| `fah-config/src/lib.rs` `validate()` | every numeric range, `MIN_CACHE_MAX_BYTES`, `MAX_UPSTREAM_SERVERS`, the two per-protocol upstream rules, `validate_ip`, `validate_nonzero_port`, `validate_allowed_destination`, the `min_ttl ≤ max_ttl` cross-field rule |
| `fah-config/src/schema/*` | every enum's **accepted** values and the integer widths |
| API.md §Session authentication / §Configuration | the `[api] tls` lock-out wording, the ≥ 12-character password rule |

`schema/*` beats CONFIGURATION.md where they differ: `dns.blocking.mode` offers
**`null_ip` and nothing else**, because that is the only variant `BlockingMode`
deserialises.

**Dirty state is per field against the last `GET /config`**, never a diff of a
re-fetch. An edit equal to the baseline **removes** the key. `config_changed`
re-reads and rebases: unsaved edits survive, and any edit the server has since
made itself stops being dirty.

**Arrays travel whole** — `config_store.rs::merge` replaces arrays rather than
merging them, so one edited upstream row sends all of `dns.upstreams.servers`.

**`RefreshRegistry.observe` is a tap**, bounded by five tested properties: no
replay on registration, no refcount, no timer, no fetch, no path to
`invalidate`. `services.ts` is the only call site.

**The restart banner is global UI state with no timer.** Armed by a
`restart_required: true` response or a `config_changed` event; cleared when
`fetchedAt − uptime_seconds·1000 > armedAtMs` — both are the client's clock and
`uptime_seconds` is a server-measured **duration**, so no skew tolerance is
needed. **No Dismiss** — a dismissed banner would leave a boot-only change
pending with nothing left to say so.

**`[api]` is present and gated.** A Save whose dirty set touches one of its
three keys opens a confirmation naming that key's consequence; Cancel sends
nothing. **No masked API key is drawn** — no endpoint returns it, so a mask
would imply this browser holds something it does not.

**The Live Feed's ring is fixed-capacity and frame-coalesced.** Allocated once
at capacity (500, or 200 on a phone, sampled once per mount); a pending
`requestAnimationFrame` through `lifecycle/timers.ts` collapses a burst into one
render per frame, which gives the hidden-page pause for free. Pause freezes the
rendered snapshot only — the ring keeps filling. The feed reads newest-first,
paged at 50 / 100 / 200, with the page index clamped at render rather than
corrected in an effect.

**The degraded explanation is one module** — `components/degraded-banner.tsx`
serves Upstreams and Health, with a source-level pin.

**Memory is drawn to `sketch/Memory.dc.html`**, which §3.1 records as the
owner-approved baseline. Composition is a stacked bar. Peak is never a slice and
no budget is a wall: both are dashed rules with captions over a track that
continues past them. `residual_bytes` is `process_rss − accounted_bytes`,
computed server-side, so the stack closes by construction and is never
re-derived client-side. Further Memory decisions:

- **Three categorical hues is the cap**, not a preference — a fourth fails the
  colour-blind floor beside the amber and the red (§5.1). Residual takes a
  texture, RSS the stack-top ink, cache/stats two steps of one hue.
- **The ramp is page-scoped.** The shared `--series-*` carries the same
  collisions; repainting four other pages is out of scope.
- **`max_points` is sent per range** — the endpoint's 1000 default silently
  decimated 24 h to stride 2 while the readout claimed every sample.
- **The y ceiling steps a ladder, never fits the data** — a fitted axis relabels
  on every refresh and renders a flat line as a mountain.
- **`/telemetry` is a third one-shot read** for `uptime` and `ruleset rules`. No
  timer, no subscription.

### 1.3 · Declared deviations from the plan

The plan's §3 resolutions **C1–C15** and its §4b registry **X1–X7** are
implemented as written. Further departures:

| # | Plan / artboard | Shipped | Why |
| - | --------------- | ------- | --- |
| 1 | KTD1's `string` control kind; `UPSTREAM_ROW_FIELDS` | Neither ships | No top-level field is a plain string, and the row editor's cells are not `FieldMeta` — the array is one patch key. Both would have been dead code (principle 14) |
| 2 | §7.2: hand the banner store `Pick<RefreshRegistry, 'observe'>` | `services.ts` makes the call and hands the store a callback | Satisfies both §7.2 bullets: the caller pin holds literally, and the store cannot reach `subscribe`/`invalidate` because it never receives the registry |
| 3 | §6 marks D3/D4 *(count)* | Both are functions in `derive.ts` | The two figures most easily got wrong, checkable in one file |
| 4 | §6 D6 and D9 as new derivations | Reuse `sliceShare` | Identical arithmetic; a second copy is principle 4 |
| 5 | §8.1 lists `[http.listen]` and `[http]` | One `[http]` card, keys still fully dotted | The artboard's nav draws one entry |
| 6 | `validate`'s `address.starts_with("https://")` | `isTlsUrl()` splits on the separator | `scripts/postbuild.mjs` refuses a shipped asset containing that literal |
| 7 | Section notes in the card's `secondary` slot | Rendered in the card body | V6: the title bar cannot shrink, and `[egress]`'s note pushed the page 676 px sideways at 900 px |
| 8 | Rail captions anchored to their marks | Captions past 55 % flip left; below 768 px they wrap at 150 px | V6: the ceiling caption ran 60 px past the body at 390 px |
| 9 | The feed draws every held row | A page of 50 / 100 / 200, newest first | Owner request. Drawing all of them costs a row per held event on every flush |

---

## 2 · Findings and resolutions

Adversarial pass over the whole p5-09 range (`ec9e9c8` → working tree). Ground
truth: task file, approved plan, `API.md`, `CONFIGURATION.md`,
`crates/fah-config`, `crates/fah-api`, `crates/fah-model`, `docs/dashboard/`.
Claims in §1 were not taken on trust.

Every finding raised across all passes is listed; **status is the current one**.

### 2.1 · Blockers — all closed

| # | Failure | Evidence | Closed by |
| - | ------- | -------- | --------- |
| **B1** | `settings/server-list.tsx` — the row key was `` `${index}-${row.address}` ``. The address is what the row's own input edits, so every keystroke remounted the row and replaced the `<input>`. The upstream address and hostname cells could not be typed into. | Keyed diffing matches by key; no test drove this control | `key={index}` — the array is positional, the index *is* the answering endpoint |
| **B2** | `settings/field-row.tsx` `DestinationList` — parsed on every `onInput` and re-rendered from `entries.join('\n')` into a fully controlled textarea, so a typed newline was stripped before the next keystroke. Adding an `egress.allow_destinations` entry was impossible. | `pages/rules.tsx` uses the same editor correctly, buffering raw text | The raw draft is held in component state and `parseDestinations` runs only on the way out; the draft is dropped when it no longer parses to what arrived from above |
| **B3** | `services.ts` + `system/restart-banner.ts` — the clear condition used `nowMs()` at announcement time against a payload that could be the previous read (`announce()` fires twice per fetch, the first carrying the retained reading). A stale reading could clear a banner with the restart still pending. | `EndpointState` already carried `fetchedAt`, and it was discarded at the call site | `observeHealth` skips `pending` announcements and judges uptime at `state.fetchedAt`; `services.ts` no longer samples a clock |

### 2.2 · Regressions and contradictions — all closed

| # | Finding | Status |
| - | ------- | ------ |
| **C1** | The review file contradicted itself after the Memory rebuild: gzip recorded at five values, tests at five counts, and V5 still read PASS while claiming Memory's entry traffic was two requests. | **Closed.** One measurement set (§6); V5 restated in §4. This consolidation removed the stale blocks. |
| **C2** | `router/routes.ts` — the `/diagnostics/memory` comment named `/debug/memory` and `/history/perf` only. `/telemetry` is a third entry read, named neither there, nor in the plan, nor in KTD5. | **Closed.** The comment names all three reads and states why `endpoints` stays `[]`: the array declares *polled* slots, and none of the three is one. |
| **C3** | The specification was edited to match the implementation — `sketch/Memory.dc.html` (608 lines touched), the task file (**including the acceptance criteria**: "the memory donut's slices sum to RSS" became "the memory composition bar's segments…") and `visual-system.md` (+33). | **Reclassified — §3.1.** Owner-approved design change, not a defect. |
| **C4** | `memory/budgets.ts` — hysteresis was dead and its one live path used the wrong threshold: the KPI card turned watch at **97 MiB** while the chart captioned its rule at **100 MiB**. | **Closed.** The watch point loosens only from `watch`/`over`, so a first reading sits on the documented 100 MiB. Completed by U1. |
| **C5** | `charts/format.ts` — `formatBytes` divided by 1024 and labelled the result `KB`/`MB`, on the one page-set whose header teaches that MB is decimal and MiB is not. | **Closed.** Labels are `KiB`/`MiB`; the divisor and every printed number are unchanged. |

### 2.3 · Sketch deviations

Read against `sketch/Memory.dc.html` as the approved baseline (§3.1).

| # | Sketch | Was | Status |
| - | ------ | --- | ------ |
| **M1** | Ruleset tile reads `43.7 % · 752,585 rules` | `compiled rules`, no count — dropped because `/debug/memory` does not carry it, a resolution made stale by the `/telemetry` read | **Closed.** Optional `rules` prop fed from telemetry, falling back to the wording when that read fails |
| **M2** | Key metrics carries a `purge delay` row | Absent | **Open, intentional.** `MIMALLOC_PURGE_DELAY` is an env var no endpoint reports (phase constraint 1). Declared in code at `key-metrics-card.tsx` |
| **M3** | `minor page faults · lifetime` = `4,211,337` | `compactCount` → `4.2M`, losing the precision the row exists for | **Closed.** `toLocaleString()` |
| **M4** | RSS and residual sparklines are gradient **area** fills | Stroke only, no `<defs>` | **Closed.** An area path per contiguous run, closed to the box floor, gradient to transparent; stops take the line's colour from CSS, so it follows the theme. A gap breaks the fill rather than bridging it |
| **M5** | Peak spark is a dashed step with a drop dot | The generic `Sparkline` over `peak_rss` | **Closed.** `PeakSpark`: step-after with a solid dot at every fall, since a fall in this series is always a restart and never a reclaim. No area — the band under a high-water mark is not a quantity |
| **M6** | Fault chart carries y labels, x ends and a `restart + compile` annotation; the figure reads `118 /s · flat` | The y-label half of the finding was **wrong** — `charts/lines.ts` already labels them. Genuinely missing: the x ends, the annotation, the descriptor | **Closed as corrected.** X ends and a measured descriptor added. The annotation is **deliberately not** added: `faultRate` drops restart-spanning samples on purpose, so there is no spike to mark |
| **M7** | The header verdict pill is always present | Returned `null` under six samples, leaving the slot empty | **Closed.** A third neutral state — `residual — not enough history` — rather than an empty slot or a steadiness the data cannot support |
| **M8** | `updated N ago` + Refresh covers the page | Refresh re-ran only `/debug/memory` and `/telemetry`; the history effect keyed on `[range]` alone, so the chart, four sparklines, the allocator window and the verdict kept the entry fetch under a timestamp that said otherwise | **Closed.** `[range, reloads]` |

### 2.4 · Fix-pass findings

| # | Finding | Status |
| - | ------- | ------ |
| **U1** | Two functions decided RSS state: `rssState` (hysteretic, KPI card only) and `stateStroke` (a y-keyed gradient with no hysteresis). The sketch attributes the 3 MiB band to the **chart line**, which was the one without it. | **Closed.** The y-keyed gradient is gone. `rssStates(values)` walks the series once carrying `previous`; the line's stroke is a gradient keyed on **x** with paired stops at each state change, and `latestRssState` reads the tail of the same walk for the card, so the two cannot disagree. |
| **U2** | The hysteresis was practically unreachable — a page visit held one reading, so the ref could only carry across a manual Refresh. | **Closed by U1** — the carry comes from the history series. |
| **U3** | The `HYSTERESIS` doc explained itself in terms of the trend chart while its only consumer was the KPI card. | **Closed by U1** — comment and consumer name the same element. |
| **T1** | "is drawn with the state the card last held" was a **source-text grep**: it passed on a variant with the hysteresis dead and failed on a rename. | **Closed.** Deleted, replaced by six behavioural cases asserting the rendered tone across successive readings. |
| **T2** | The `pending` guard in `observeHealth` has no test that fails without it — the `fetchedAt` half rejects the case on its own. | **Open, non-blocking.** The guard is correct; the test does not isolate it. |
| **T3** | "drops the draft when the baseline replaces it" would have passed pre-fix, and its `postBodies()` assertion is vacuous. Its title names a baseline replacement while the body exercises a Discard. | **Open, non-blocking.** It guards a plausible future bug, which is its real value. |
| **T4** | One keystroke in the address cell of a one-row fixture. Untested: the hostname cell, a multi-row list, the positional shift after a delete. | **Open, non-blocking.** |
| **T5** | Four of five C4 cases exercised the pure function; the user-visible symptom was asserted nowhere. | **Closed by T1's replacement**, which asserts the rendered tone. |

### 2.5 · Render defects — found by the browser check, not by review

None is reachable by a unit suite; all were found by rendering the page.

| # | Defect | Fix |
| - | ------ | --- |
| **R1** | The peak line struck the threshold captions through wherever a restart landed near a threshold — the captions were drawn under the series. | Captions moved from `drawClear` to `draw`, each on a measured background plate. `ChartTheme` gained `surface`: canvas text has no z-order, so an annotation over a series must clear its own ground. |
| **R2** | `restart resets it` was clipped mid-word on the four-up KPI row below ~1400 px; `new peak … never sampled` ran off the plot's right edge. | The KPI caption wraps to its mark's width; the canvas caption clamps inside the plot. |
| **R3** | The last x tick collided with `now` on 7 d and 30 d. | The final tick label is dropped inside `now`'s gutter. `now` survives — it names the end of the window, which no tick does. |

**Also checked, not defective — the range chips.** Reported as "only changing
the horizontal line". Three clicks issued three distinct requests (spans **15 h /
159 h / 711 h**, `max_points` 1440 / 5000 / 5000) and every range-labelled card
followed; the 30 d view redraws the stack on a monthly axis. A window where RSS
looks unchanged across all three is a history table holding less than the
window. The KPI's RSS figure comes from `/debug/memory` and is range-independent
by design — it is *this instant*, which no window changes.

### 2.6 · Duplication — N1–N4 closed

| # | Finding | Closed by |
| - | ------- | --------- |
| **N1** | `memory/trend-card.tsx` recomputed `stats_aggregates_bytes + stats_clients_bytes` inline beside `statsBytes` (D5). | Calls `statsBytes` |
| **N2** | `uptimeLabel` duplicated `time.ts` `formatUptime` **and** its format (`4 h 31 m` vs `4h 31m`), so one reading read differently on Health and Memory. | Deleted; the row calls `formatUptime`. Rendering change recorded in §3.2 |
| **N3** | Threshold captions hard-coded `100 MiB` and `122.1 MiB`. A budget change would have made the caption lie. | Formatted from `WATCH_THRESHOLD` and `STEADY_STATE_BUDGET`; the trend card's footnote takes the watch point the same way |
| **N4** | `'128 MB'` and `'▏128 MB budget'` were literals beside a `STEADY_STATE_BUDGET / 1_000_000` derivation. | One `budgetLabel(bytes)` in `budgets.ts`, used by `STEADY_STATE_LABEL`, `CEILING_LABEL`, the rail's right end, the peak tick caption and both Key-metrics rows |

After this pass, `grep "128 MB\|256 MB\|122\.1 MiB\|100 MiB"` over non-test
sources returns only prose inside doc comments. **No rendered string carries a
threshold or a budget literal.**

### 2.7 · Non-blocking findings — open, recorded

Not fixed. None blocks the task; each is a real observation with its evidence.

**Duplicated logic**

| # | Where | Note |
| - | ----- | ---- |
| N5 | `pages/diagnostics-memory.tsx` | Re-implements `performance/use-perf-history.ts` `perfQuery`'s window arithmetic; a `fields`/`maxPoints` parameter would have covered both. |
| N6 | `api/types.ts` `PerfMemory` | Re-declares the wire struct already flattened onto `Memory`, with `residual_bytes` non-null in one and `number \| null` in the other. |

**Dead code, dead branches, stale comments**

| # | Where | Note |
| - | ----- | ---- |
| N7 | `settings/patch.ts` | `String(octet) === String(Number(part))` is a tautology, so the intended leading-zero rejection never runs: `01.0.0.1` passes a check whose own comment says it "must never accept what the server rejects". Rust's `IpAddr` parse refuses it. |
| N8 | `memory/trend-options.ts` `rule()` | `if (value > max) return` is unreachable — the y range never returns less than 160 MiB, so `max` always exceeds both thresholds. |
| N9 | `memory/trend-options.ts` | Orphaned doc block ("The watch zone and the two threshold rules…") sits above `restartsOf`, which it does not describe. |
| N10 | `settings/password-dialog.tsx` | The `done` branch is unreachable: `onSignedOut()` navigates in the same tick as `setDone(true)`, so the "every session was signed out" state never paints. Plan §8.1 asks for it. |
| N11 | `styles/components.css` | "hidden means out of the tree" is false — the rule is `display: none`, so every flush builds both the nine-column table **and** the phone card list. This doubles exactly the per-flush cost the rAF coalescing exists to bound. |
| N12 | `settings/metadata.ts`, `settings/patch.ts`, `memory/budgets.ts`, `live-feed/ring.ts` | Several exports with no consumer outside their own module. Not dead code, but public surface nothing asks for. No genuinely orphaned export was found. |

**Correctness edges the tests do not reach**

| # | Where | Note |
| - | ----- | ---- |
| N13 | `memory/composition-card.tsx` + `derive.ts` `stackedMemory` | `residual` is `saturating_sub` server-side and the model carries an explicit `over_accounted()` predicate. In that state the four shares sum past 100 % and the stacked chart's top band inverts, while the card states "the four components **sum to RSS exactly**" unconditionally. |
| N14 | `pages/settings.tsx` | The single-flight `readConfig` can join an entry read that started **before** a `config_changed` arrived, adopting a pre-change baseline nothing re-reads. |
| N15 | `derive.ts` `upstreamStateCounts` | `counts[upstream.state] += 1` on an unmodelled state yields `NaN` and adds a key. Invisible, because the rendered line reads only the three known states — which is why the "invents no fourth state" test passes. |
| N16 | `settings/raw-panel.tsx` | The unmodelled-key count excludes `rules.lists` but counts every `policies.*` leaf, inflating the figure with keys that are `422` by design. |
| N17 | `pages/live-feed.tsx` | Row keys are `` `${row.ts}-${pageIndex}` ``; the index is page-relative and shifts as rows arrive, so keys change meaning between flushes. |
| N18 | `shell/shell.tsx` | The top-bar group prefix is a literal `'diagnostics' → 'Diagnostics'` map. A second nested group loses its prefix silently. |
| N19 | `memory/trend-options.ts` | `hatchPattern` builds a `<canvas>` and a `CanvasPattern`, and `stateStroke` a `CanvasGradient`, on **every** draw. Not profiled. |
| N20 | `memory/trend-options.ts` | `restartsOf` reads `u.data[5]` positionally. Adding a series re-targets restart detection at the wrong line, and nothing asserts the layout. |
| N21 | `pages/diagnostics-memory.tsx` | `/telemetry` is fetched raw, bypassing the shared registry that may already hold a fresh reading (Health → Memory pays twice). Not a timer, so the phase invariant holds; the shared-mechanism rule bends. |

**Test quality**

| # | Where | Note |
| - | ----- | ---- |
| N22 | `styles/series-palette.test.ts` | "keeps every fill above its own card" checks 3 of 6 fill tokens; only `--memory-peak`'s exclusion is explained. |
| N23 | `refresh/observe-callers.test.ts` | Comment says "exactly two places", assertion is `toHaveLength(3)`. The assertion is right; the prose is not. |
| N24 | `pages/settings.test.tsx` | No test drives `UpstreamServers` or `DestinationList` — the two controls carrying B1 and B2. Their *validation* is covered thoroughly, which is why both bugs survived a green suite. |

### 2.8 · Verified sound

Checked and found correct; recorded so a later pass need not re-derive it.

- **`RefreshRegistry.observe` cannot become a lifecycle path.** The observer map
  is written only in `observe()` and read only in `announce()`; `announce` is
  called from `fetch` alone; `subscribe` performs no replay; no path reaches
  `invalidate`. `observe-callers.test.ts` is a real source pin.
- **Changed-keys-only writes.** `buildPatch` emits only `Object.keys(edits)`,
  `setEdit` deletes a key returning to baseline, `rebase` drops an edit the
  server made itself, and arrays travel whole — matching `config_store.rs::merge`.
- **Every bound, enum and mutability class in `metadata.ts`** was checked field
  by field against `BOOT_KEYS` (14 entries), `validate()` and `schema/*`. All
  correct, including the four `live` keys and `dns.blocking.mode = null_ip` only.
- **`422` anchoring** matches `fah-config/src/error.rs` exactly; the three
  by-design rejections carry no such prefix and fall through to a form error.
- **Auth redaction.** `GET /config` cannot carry `auth`, and the panel strips it
  anyway. No masked key value is drawn.
- **Health and Memory provenance.** Every displayed figure traces to `/health`,
  `/telemetry`, `/lists`, `/debug/memory`, `/history/perf` or the one-shot
  `/config`. Residual is never re-derived client-side, peak is a segment
  nowhere, the allocator pair is tagged and charted nowhere, and `null` renders
  `—`, never `0`.
- **`MEMORY_PERF_FIELDS`** is pinned, disjoint from `PERF_FIELDS`, and
  `max_points` is sent only by Memory.
- **Live Feed.** `query` is declared by that route alone, the ring is allocated
  once at capacity, `FeedBuffer` schedules at most one frame, no `setTimeout` /
  `setInterval` / bare `requestAnimationFrame` exists in the page tree, and
  unmount disposes the pending frame. The verdict vocabulary is closed and an
  unknown verdict joins no tone.
- **Route lifecycle.** No `built: false` row remains and `pages/system.tsx` has
  no referrer left.
- **No p5-08 invariant was weakened** — `derive.ts`, `lines.test.ts`,
  `literal-colours.test.ts` and `series-palette.test.ts` are additive only, zero
  deleted assertions across the range.

### 2.9 · Two defects the verification pass missed

Found by the owner reading the running page. The checks probed **mechanics**
(subscribe frame, socket close, ring bound, coalescing, pause, overflow) and
never asserted that the feed *reads* correctly.

| # | Defect | Fix |
| - | ------ | --- |
| 1 | **The Live Feed rendered oldest-first.** The ring's `items()` is oldest-first and the page rendered it verbatim. | One `.reverse()` at the render boundary, with a rendered-order test over both the table and the phone cards |
| 2 | **`FeedBuffer` stopped scheduling under a synchronous scheduler.** `pending` was assigned *after* the callback that clears it, so with an inline scheduler it stayed non-null and no later push scheduled a flush. A real animation frame hides this entirely. | A `scheduled` flag set **before** the call, the cancel stored only if the callback has not already run |

**The lesson, and §2.5 repeats it: this page-set's failures are layout, paint
and reading order. The unit suite cannot see any of them. Render it.**

Also fixed here: `grid-tracks.test.ts` located its phone rules by
`lastIndexOf('@media (max-width: 767px)')`. This task added a second such block
and four checks silently began reading the wrong one. The helper now
brace-balances **every** `≤ 767 px` block and joins them.

---

## 3 · Standing decisions

### 3.1 · C3 — the sketch is an owner-approved baseline

C3 recorded that the sketch, the task file and `visual-system.md` were edited
during implementation. **The owner has stated this was a deliberate design
change they approved**: the Memory screen was redesigned mid-implementation
because the earlier design did not meet the intended direction.

`docs/dashboard/sketch/Memory.dc.html` **as it stands in the tree is the
approved baseline**, and the Memory page is required to match it. C3 is
therefore **not an implementation defect** and not a request to revert. What
remains true: "meets the original acceptance criteria" cannot be checked against
a spec written after the fact — the criteria that stand are the ones in the tree.

### 3.2 · The uptime format — deviation accepted

Closing N2 required choosing one of two formats. Memory moved:

| | Before | After |
| --- | --- | --- |
| Memory · uptime | `4 h 31 m` | `4h 31m` |
| Health · uptime | `4h 31m` | unchanged |

`Memory.dc.html:327` draws `4 h 31 m`, so the page no longer matches on that
row. **Owner decision (2026-08-29): accepted.** The shared formatter wins and
that sketch row is superseded. Not to be reverted.

---

## 4 · Verification contract

Chromium via Playwright against `fastadhunter:p5-05` in Docker on the dev box
(`fah-p506`, published on 18443). **The RB5009 was not touched.**

| # | Result |
| - | ------ |
| **V1** | **PASS.** Entering the feed sent exactly `{"subscribe":["query"]}`, socket `open`. Leaving for Health sent no smaller-union subscribe (the union empties) and the socket read `closed`. Re-entering restarted the ring at **0 / 500** — it retains nothing. |
| **V2** | **PASS.** With the feed closed, a second bearer socket subscribed to `stats` only received **28 stats frames and 0 query frames** over ~56 s, spanning 30 real DNS queries. Delivery and the engine-side gate observed together. |
| **V3** | **PASS.** A boot-only edit sent exactly one key, answered `restart_required`, and armed the banner naming it. A two-key save answered both flags; a runtime-only edit answered "Applied live." with no restart sentence. After `docker restart`, a route change to `/cache` left the banner up (it does not clear from an unrelated page) and returning to Settings **cleared** it. Persistence confirmed through `GET /api/v1/config`. |
| **V3a** | **PASS.** `api.tls` toggled: **0** requests before confirmation, the dialog naming "signing in stops working" and "bearer", **0** after Cancel, dirty set cleared on revert. |
| **V4** | **PASS.** `visibilityState` forced hidden: the socket stayed open, then read `closed` after the 30 s grace. Returning reconnected and re-subscribed; the 68 held rows survived and what arrived while closed is simply missing. |
| **V4a** | **PASS.** A 60-query burst produced **5** DOM mutation batches, not 60. Pause: the ring moved 60 → 68 while the rendered table stayed at 60; Resume rendered all 68. Clear emptied both at once. |
| **V5** | **PASS, restated.** `fahTimers()`: Settings **0**, Health **3** (its three declared endpoints), Memory **0**, Live Feed **0**. Sockets `closed` on Health and Memory. **Memory makes three one-shot reads on entry** — `/debug/memory`, `/telemetry`, `/history/perf` — none of them polled, and after the M8 fix **Refresh re-runs all three**. The original wording named two reads and was wrong; the invariant it existed to protect (no timer, no subscription) is intact, and is now asserted in the suite by *holds no timer and subscribes to no event type* and *re-reads the history too when Refresh is pressed*. |
| **V6** | **PASS, after two fixes.** 1400 / 900 / 390 px × both themes × four routes: `scrollWidth − clientWidth` **0** on both `documentElement` and `.main`, 24 measurements. At 390 px the feed is one card per event, the bar is three 102 × 44 controls, and the chips are 44 px tall in their own scroller. The rotate dialog is `aria-modal`, focus lands inside, Escape closes it, and it issues no request until confirmed. Two real defects found here — Deviations 7 and 8 in §1.3. |
| **V7** | **PASS.** The budget gate is green and the postbuild forbidden-content scan passes (no external URL, no dev-gallery marker, no Pi-hole string). |
| **V8** | **PASS — re-run after the ordering fix and the pager.** 120 queries driven in order: the feed reads `q120` down to `q71`, `rows 1–50 of 120 · page 1 of 3`, `Newer` disabled at the live edge. `Older` walks 1 → 2 → 3 and stops; the size chips redraw at 100 and 200 and return to the newest page. At 390 px: 200-row ring, one card per event newest-first, **0** horizontal overflow. |

---

## 5 · Measurements

### 5.1 · Colour-blind separation — why three hues

Candidate fourth hue against the first three plus amber/red. Floors: 8 simulated
models, 15 normal vision (OKLab ×100).

| Fourth hue | Worst pair | ΔE | Model |
| --- | --- | --- | --- |
| magenta `#d55181` | vs aqua | 1.6 | deuteranopia |
| violet `#9085e9` | vs blue | 1.9 | protanopia |
| lime `#7fae2a` | vs amber | 2.4 | deuteranopia |
| teal `#1f9dbb` | vs blue | 10.0 | normal vision |

Shipped three hues, all pairs: worst normal-vision ΔE **20.9 dark / 24.0 light**.
The pre-existing app ramp measures ΔE 9.8 (cyan vs blue), and light mode
resolved residual and stats to the same hex.

### 5.2 · Render check — Memory, final

`/diagnostics/memory` at **390 / 900 / 1296 / 1440 px × dark and light**, on the
built bundle via `vite preview`, against stubbed API responses.

| Check | Result |
| ----- | ------ |
| Horizontal overflow | **0** at every width and theme |
| Spark marks | 2 gradient areas, 3 spark lines, 1 restart drop dot, in all eight |
| Fault card | `118 /s · flat`, with the `24 h ago` / `now` ends present |
| Composition | `43.7 % · 752,585 rules` present |
| Card titles | single-line, unclipped |
| Tooltip, 41-point sweep | no clipping either edge; exactly one side change, at the midpoint |
| Range ticks | `03:00…` / `22 Sat…` / `Jul 30, Aug 2…` |

The only console errors were `/health` 502s — the stub covers `/api/**` and the
shell's liveness probe is not under it. A fixture gap, not a page fault.

---

## 6 · Gates

The one measured set, taken on the working tree after the final pass. It
supersedes every figure this file carried earlier.

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace` | **1,196 passed, 0 failed** |
| `npm run typecheck` | clean |
| `npm run test` | **897 passed, 54 files, 0 failed** |
| `npm run build` | **127,535 B gzip** — 83.0 % of the 153,600 B budget; brotli **113,083 B** |
| `git status -- crates/` | empty — no `.rs` changed, so the cargo gates are unaffected by this task |

Bundle context: p5-08 shipped 96,408 B gzip. The four pages, the shared
infrastructure and the Memory rebuild account for the difference; headroom for
p5-10 is **26,065 B**.

---

## 7 · Known limitations and deferred items

| # | Item |
| - | ---- |
| 1 | **The restart banner forgets on a hard reload** — the arming is in-memory (KTD3). The API offers no "pending boot-only changes" read. The next save re-arms it. |
| 2 | **The `config_changed` listener is not driven in vitest** — `SocketManager` exposes no emit seam. Its two effects are covered separately; live, the single-flight join keeping the pair to one `/config` was not separately counted in the browser. |
| 3 | **Not exercised live, unit-tested only:** the raw panel's "none configured" branch, the `degraded` banner on Health, and the RSS-unavailable state (the container is Linux). |
| 4 | **`stride > 1` unreachable on this fixture**, as in p5-08's V11. |
| 5 | **Heap retention not measured** across navigation rounds — without a forced GC that is evidence in neither direction. |
| 6 | **Metadata drift is the accepted cost** of the hand-written form. Mitigated by the per-field `source` anchors and by the raw panel making an unmodelled key visible. |
| 7 | The plan's §13 documentation proposals are **proposals only** and are not applied. |
| 8 | **The pager's page size is per-visit**, like the range chips on Performance and Memory. |
| 9 | `--memory-peak` is 2.17 : 1 on the light card. Darkening walks it to ΔE 3.0 from the status red under deuteranopia. Relieved by legend, readout and KPI figure; the exclusion is asserted in `series-palette.test.ts`. |
| 10 | Health and Live Feed were not re-opened after the top-bar change. |
| 11 | Annotation captions are canvas text — invisible to screen readers and to text search. Every figure they name is DOM elsewhere. |
| 12 | The `7 d` tick reads `22 Sat`, not `Sat 22` — `Intl` locale order. |
| 13 | `watch` at 100 MiB is a judgement, not a measurement — a constant in `budgets.ts`. |
| 14 | `ResidualVerdict` compares window thirds at 10 %: a shape statement, not a leak detector. Under six samples it now says so (M7). |

---

## 8 · Verdict

**PASS.**

Closed: **B1**, **B2**, **B3**, **C1**, **C2**, **C4**, **C5**, **M1**, **M3**,
**M4**, **M5**, **M6**, **M7**, **M8**, **N1**–**N4**, **U1**–**U3**, **T1**,
**T5**, **R1**–**R3**, and the two reading defects in §2.9.

Reclassified: **C3** — owner-approved baseline change (§3.1).

Open, none blocking: **M2** (the sketch's `purge delay` row, correctly absent —
no endpoint reports `MIMALLOC_PURGE_DELAY`), **N5**–**N24** and **T2**–**T4**.
The uptime deviation in §3.2 is decided, not open.

---

## 9 · Non-blocking pass — N7, N10, N11, N13, N14, N15, N17, N18, N19, N20

Scope: **those ten findings only.** N5, N6, N8, N9, N12, N16, N21–N24, M2 and
T2–T4 untouched. No endpoint, no new figure, no API change.

### 9.1 · Finding → fix

| # | Fix | Where |
| - | --- | ----- |
| **N7** | `part === String(octet)` in place of `String(octet) === String(Number(part))`. The raw text is compared to its own parsed value, which is what rejects a leading zero; the old form compared a number to itself and never ran. | `settings/patch.ts` `looksLikeIp` |
| **N10** | The success path sets `done` and stops. `onSignedOut()` now fires from a **Sign in again** button in the done panel, so the sentence explaining the secret rotation is reachable and the operator acknowledges it. No timer was introduced. | `settings/password-dialog.tsx` |
| **N11** | The page builds **one** tree: the table at a wide viewport, the cards at a narrow one. Chosen from the same one-shot `matchMedia` sample that sizes the ring, through `ringCapacity`, so the bound and the layout cannot disagree. The CSS rules stay as the second line of defence. | `pages/live-feed.tsx` |
| **N13** | Two halves. `stackedMemory` gives an over-accounted row a **gap** — a row that does not sum to RSS is not a sample of a stack that claims to, and drawn as one the top band falls below the band under it. The composition card computes `accounted > rss` and swaps its footnote: it names the accounting bug instead of asserting the identity that fails in exactly that state. | `derive.ts`, `memory/composition-card.tsx` |
| **N14** | `readConfig(signal, startedAfter?)`. A read provoked by a change joins an in-flight one **only if that request began after the change**; the `config_changed` handler passes the announcement instant and the save passes the instant its `POST` resolved. The join survives for genuinely concurrent reads, which is the race it was built for. | `pages/settings.tsx` |
| **N15** | `if (upstream.state in counts)` — a state outside the vocabulary is ignored rather than writing `NaN` and adding a key. | `derive.ts` `upstreamStateCounts` |
| **N17** | `BoundedRing` counts what it has been pushed and exposes `firstSequence`; `FeedBuffer` hands it to the flush callback, and the page carries `{ row, seq }` through the reverse, the filter and the pager. The row key is that sequence. | `live-feed/ring.ts`, `live-feed/filters.ts`, `pages/live-feed.tsx` |
| **N18** | `GROUP_LABELS: Record<RouteGroup, string>` in `routes.ts`, read by the top bar and the sidebar. Adding a group now fails to compile until it is named, where the `'diagnostics' → 'Diagnostics'` conditional would have dropped the prefix in silence. | `router/routes.ts`, `shell/shell.tsx`, `shell/sidebar.tsx` |
| **N19** | A `PaintCache` per options object. The hatch is rebuilt only when the device ratio moves; the state gradient only when the readings or the plot box do. Both were being rebuilt on **every draw**, which on this chart means every cursor move. | `memory/trend-options.ts` |
| **N20** | A `SERIES` constant naming every column. The series array is assembled from it by name, the bands are expressed in it, and every `u.data[…]` reader uses it — so the layout and its readers cannot drift apart. | `memory/trend-options.ts` |

### 9.2 · Two changes that alter behaviour, deliberately

Both are the fix, not a side effect.

| # | Before | After |
| - | ------ | ----- |
| **N10** | The dialog navigated to `/login` in the same tick as the success, so the done panel never painted. | The panel paints and the operator dismisses it. One extra click on a path taken rarely, in exchange for the only acknowledgement the rotation gets. |
| **N14** | A save or an event re-read always joined an in-flight `GET /config`. | It joins only a request that began after the change. **When `config_changed` for this browser's own write arrives after the save's re-read has started, the pair now costs two requests rather than one.** Correctness over the saved request; the ordinary case is unchanged and still asserted. |

### 9.3 · Tests

Nine added, **909** total.

| Test | Asserts |
| ---- | ------- |
| `looksLikeIp` · leading zero | `01.0.0.1`, `192.168.010.5`, `1.2.3.04` refused; `0.0.0.0` and `10.0.0.1` still accepted |
| `upstreamStateCounts` · unknown state | ignored, no `NaN`, no fourth key |
| `stackedMemory` · over-accounted | all four bands `null` for that row |
| `stackedMemory` · exact sum | the boundary the gap must not swallow — equality is the identity holding |
| composition · over-accounted | prints *claim more than RSS*, never *sum to RSS exactly* |
| composition · normal | the claim survives when the identity holds |
| feed · one tree, wide | the table renders and `.feed-cards` is absent |
| feed · one tree, narrow | the cards render, `.feed-table` is absent, and the ring reads `2 / 200` — one sample, both consumers |
| feed · row identity | a row keeps its DOM node when a new event shifts its index |
| password dialog | the done panel paints with its **Sign in again** action |
| plot options | the six series sit in the slots `SERIES` names |
| group label | every route's group has a label, and the table holds exactly one |

**Pre-fix proof.** Three fixes were reverted and their tests re-run: **3 failed**
— `01.0.0.1: expected true to be false`, the `NaN` count, and the over-accounted
row drawn as a stack. The feed's three cases fail against the old page too (the
old one rendered both trees, which is what the replaced test asserted).

**No new test for N14.** Driving the race needs a socket emit seam
`SocketManager` does not expose, and the save-before-first-read ordering is
unreachable — the form cannot be edited before the baseline lands. The guard
against the change's own risk is the existing *re-reads it once after a save*
case, which still passes: the ordinary pair still collapses to one request.

**No new test for N19.** The cache is a performance property; asserting "built
once" would pin the mechanism rather than the behaviour, and jsdom draws no
canvas. The rendered output is unchanged and covered by the existing chart
cases.

### 9.4 · Gates

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | 54 files, **909 passed**, 0 failed |
| `npm run build` | **128,100 B gzip** — 83.4 % of the 153,600 B budget; brotli **113,576 B** |
| `git status -- crates/` | empty — no `.rs` changed |

Bundle moved 127,535 → 128,100 B gzip (**+565 B**) for the ten fixes.

### 9.5 · Verdict

**PASS.** All ten closed.

Still open, unchanged and non-blocking: **N5**, **N6**, **N8**, **N9**, **N12**,
**N16**, **N21**, **N22**, **N23**, **N24**, **T2**–**T4**, and **M2**. §8's
verdict stands with those ten struck from its open list.

---

## 10 · Regression pass over the fix passes

Scope: the **working tree only**, read for regressions the §2 and §9 fixes
introduced. No redesign, and the deferred rows (N5, N6, N8, N9, N12, N16,
N21–N24, T2–T4, M2) were not touched. Surfaces re-checked below.

### 10.1 · Findings

| # | Severity | Where | Regression |
| - | -------- | ----- | ---------- |
| **G1** | **Medium** | `pages/live-feed.tsx:63`, `styles/components.css:4434,4558` | **N11 made a mid-visit resize render an empty feed.** `narrow` is a `useState` initialiser — one `matchMedia` sample at mount, no listener — and it now chooses *which tree is built*. Cross 767 px without leaving the page and the built tree is the one CSS hides: desktop→phone renders the table under `.feed-scroll { display: none }` with no `.feed-cards` beside it, phone→desktop the mirror. The pager still reads `rows 1–50 of 120` over a blank body. Before N11 both trees existed and the CSS alone decided, so a resize was correct. The one-shot sample was previously invisible (X6 sized the ring only); N11 gave it a visible consequence. |
| **G2** | **Medium** | `pages/settings/password-dialog.tsx:34,72`, `settings/access-card.tsx:86` | **N10 made the sign-out escapable.** The done panel is inside `useFocusTrap(true, …, onClose)`, and `onClose` is `setDialog(null)` — not `onSignedOut`. Escape after a successful rotation dismisses the panel and leaves the operator on Settings holding a revoked cookie: stale form, no redirect, and the next request is the first sign that the session is gone. The pre-fix code navigated in the same tick, so this state was unreachable. The **Sign in again** button is the only path that navigates. |
| **G3** | **Low–Medium** | `pages/settings.tsx:66–89` | **N14 dropped the single-flight guarantee without an ordering guard on `adopt`.** The join is now conditional, so two `GET /config` can be in flight at once (a second `config_changed` at `t2` cannot join a read started at `t1 < t2`). `adopt` has no generation stamp, so whichever *resolves* last wins the baseline — which need not be the one that *started* last. Before the fix the unconditional join made at most one read in flight, and adopt order was start order by construction. The failure is a baseline one change behind, with nothing left to re-read it — the same class of defect N14 was closing, moved from "joins too eagerly" to "resolves out of order". A `startedAt` carried into `adopt` and compared against the last adopted stamp closes it. |
| **G4** | **Low** | `derive.ts:453`, `pages/memory/kpi-rail.tsx:50` | **N13 broke U1's stated invariant in exactly the state N13 exists for.** U1 closed on "`latestRssState` reads the tail of the same walk, so the card and the line cannot disagree". After N13 they no longer walk the same values: the chart's RSS series is `stackedMemory`'s band, which is now `null` on an over-accounted row, while the KPI card walks raw `item.rss_bytes` plus the live reading. On an over-accounted sample the line carries the state forward across a gap and the card reads that sample directly, so the two can differ. Both predicates agree on *what* over-accounted means (`ruleset + cache + stats.total()` matches `MemoryComponents::accounted`, checked against `fah-model/src/memory.rs:138`); it is the state walks that diverged. |
| **G5** | **Low** | `shell/sidebar.tsx:61,103` vs §9.1 N18 | **The N18 claim overstates what the fix guarantees.** `GROUP_LABELS` does force a label for a new group, but the sidebar still hardcodes `route.group === 'diagnostics'` for the filter and `GROUP_LABELS.diagnostics` for the heading. A second group compiles once labelled and is dropped from the sidebar in silence — the failure mode N18 says the fix removed, moved from the top bar to the sidebar. The top-bar half is genuinely fixed. |

### 10.2 · Verified — no regression found

| Surface | Checked |
| ------- | ------- |
| Settings editing / rebase / dirty | `readConfig` join, `adopt` → `rebase`, `setEdit` delete-on-baseline, `buildPatch` changed-keys-only, `discard`, `send`'s `setEdits({})` before the re-read. B1 (`key={index}`) and B2 (raw draft, `parseDestinations` on the way out) both intact; N7's `part === String(octet)` rejects `01.0.0.1` and still accepts `0.0.0.0` |
| Restart-banner lifecycle | Arm on `restart_required` and on `config_changed`; re-arm takes the later `armedAtMs` and the key union; B3 holds — `observeHealth` still skips `pending`/`error` and judges at `state.fetchedAt`, and `registry.ts:260` stamps `fetchedAt` before the settle announcement. An aborted refresh announces a **consistent** (data, `fetchedAt`) pair, so no stale-clock clear is reachable. `services.ts` samples no clock |
| Memory chart / state consistency | `SERIES` covers every `u.data[…]` reader; `[xs, ...bands, peak]` matches the constant; bands `rss→stats→cache→ruleset` stack in order; `PaintCache` is per options object and the options memo is `[theme, range]`, so a theme change rebuilds both cached paints; `stateStroke`'s `values` key is the stable inner array uPlot holds; `restartsOf` reads `SERIES.peak`, which N13's gap does not touch; M8's `[range, reloads]` re-reads all three on Refresh; `MAX_POINTS` (1440/5000/5000) is inside `MAX_PERF_POINTS = 5_000` |
| Live Feed identity / order / pagination | `firstSequence = pushed − filled`; `clear()` deliberately does not reset `pushed`, so sequences never repeat after a Clear; the `{row, seq}` pair survives the reverse, the filter and the pager; `applyFilters` copies before `.reverse()`, so the `rows` snapshot is never mutated; pause/resume/clear each move `rows` and `firstSeq` together; §2.9's `scheduled`-before-call ordering is intact and the cancel is stored only when the callback has not already run; page index clamped at render |
| Route lifecycle | No `built: false` row; no referrer to the deleted `pages/system.tsx`; `components/degraded-banner.tsx` has no stale importer; `components/donut.tsx` is at HEAD and still owned by the Cache page; Health declares three endpoints, Memory and Live Feed none |
| Refresh behaviour | `observe` is read in `observe()` and `announce()` only; no replay, no refcount, no timer, no path to `invalidate`; `announce`'s new `endpoint` parameter changes no subscriber path |
| Shared shell | `ContentHeader`'s `glyph` slot leaves the twelve glyph-less pages byte-identical; `TopBar`'s prefix is additive; `RestartBanner` sits above the routed content and outside the page tree |
| Gates | `npm run typecheck` clean · `npm run test` **54 files, 909 passed, 0 failed** · `git status -- crates/` empty |

### 10.3 · Verdict

**PASS WITH DEFERRED FINDINGS.** No blocker. G1 and G2 are user-visible
behaviour the fix passes introduced and are worth closing before the task
moves; G3–G5 are recorded.

### 10.4 · Fix pass — G1–G5

Scope: **those five findings only.** No endpoint, no new figure, no API change,
and no deferred row touched.

| # | Fix | Where |
| - | --- | ----- |
| **G1** | **The bound stays sampled once; the layout follows the viewport.** `matchesNarrow()` and `observeNarrow()` sit beside `NARROW_QUERY`; the page keeps `openedNarrow` for `ringCapacity` — a memory bound, and resizing a window is not a reason to reallocate one — and holds a second, subscribed `narrow` for the tree it builds. A `MediaQueryList` with no `addEventListener` (the shape the tests stub, and any environment without one) subscribes to nothing and leaves the mount-time answer standing. The notice now reads *a desktop one* rather than *this viewport*, which the mount-time figure can no longer claim | `live-feed/ring.ts`, `pages/live-feed.tsx` |
| **G2** | `useFocusTrap(true, …, done ? onSignedOut : onClose)`. Escape still means "leave this dialog"; once the rotation has landed, leaving it *is* the sign-out, because the request that succeeded revoked this browser's cookie. The trap reads `onEscape` through a ref, so the swap as `done` flips needs no remount | `settings/password-dialog.tsx` |
| **G3** | `readConfig` returns the read — `{ run, startedAt }` — and `adopt(fresh, startedAt)` drops a document older than the newest already adopted. The conditional join lets two reads be in flight, and two requests settle in either order; without the stamp the one that *resolved* last won the baseline even when it *left* first. A join hands back the pending slot, so a joiner adopts under the instant that request actually left. Equal stamps still adopt — two answers to the same instant | `pages/settings.tsx` |
| **G4** | An over-accounted row gaps its **three component bands** and keeps its RSS. The inverted stack is what the gap exists to prevent, and RSS is not the figure in doubt: it is read from `/proc/self/status` and is what the components failed to add up to. Keeping it restores U1 — the chart's state walk and the KPI card's now see the same readings — and leaves the composition card's footnote swap untouched | `derive.ts` `stackedMemory` |
| **G5** | The sidebar names no group. Groups are collected off the route table, each drawn by a `Group` block under the section of its own routes, at the landing route and prefix derived from its members' paths, with `GROUP_LABELS[group]` and `<Icon name={group} />`. `DIAGNOSTICS_ROOT` and `inDiagnostics` are gone | `shell/sidebar.tsx` |

**Behaviour deliberately changed.** G1: dragging the window across 768 px now
re-renders the feed in the other layout instead of leaving the body empty — and
adds the application's **one** viewport listener, scoped to this page and
released on unmount. G2: Escape on the done panel navigates to `/login` rather
than returning to Settings.

#### Tests — six added, **915** total

| Test | Asserts |
| ---- | ------- |
| feed · crosses the breakpoint | the table gives way to the cards and back, with the rows intact |
| feed · bound across a resize | the ring still reads `1 / 500` after the layout has switched |
| password dialog · Escape | the done panel survives Escape and the path becomes `/login` |
| `stackedMemory` · over-accounted | the three component bands are `null`, RSS is kept |
| `stackedMemory` · RSS kept | a healthy row beside an over-accounted one: `[90, 50]` on RSS, `[60, null]` on stats |
| route table · group sprite | every `GROUP_LABELS` key is a symbol id in `sprite.svg` — the sidebar draws the key as the glyph |
| route table · group section | a group's routes all sit in one section, which is where the sidebar places it |

**Pre-fix proof.** Three fixes were reverted and their tests re-run: **4
failed** — the table still rendered after the resize, the done panel was gone
after Escape, and the over-accounted row's RSS came back `null`. G5 has no
pre-fix failure by design: it is a future-group hazard, and the two new
assertions are the pins the generic block rests on, with the existing sidebar
cases proving the rendering is unchanged.

**No new test for G3.** Driving two overlapping `GET /config` needs the
`SocketManager` emit seam §7 item 2 records as absent, and the save path cannot
be made to overlap itself from the outside. The guard against the change's own
risk is the existing *re-reads it once after a save* case, which still passes.

#### Gates

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | 54 files, **915 passed**, 0 failed |
| `npm run build` | **128,363 B gzip** — 83.6 % of the 153,600 B budget; brotli **113,765 B**; postbuild scan passes |
| `git status -- crates/` | empty — no `.rs` changed |

Bundle moved 128,100 → 128,363 B gzip (**+263 B**).

#### Verdict

**PASS.** G1–G5 closed. Still open, unchanged and non-blocking: **N5**, **N6**,
**N8**, **N9**, **N12**, **N16**, **N21**–**N24**, **T2**–**T4** and **M2**.

### 10.5 · Narrow re-review of G1–G5

Owner-specified checks, one per fix. Each is now a test rather than a reading of
the source; the three that could fail against the pre-fix code were run against
it. **No new finding.**

| Check | Result |
| ----- | ------ |
| **G1** desktop → mobile → desktop in one mount | **PASS.** The table gives way to the cards and back, with the row intact at both ends |
| **G1** ring capacity unchanged | **PASS.** `1 / 500 rows held` at the narrow end *and* on the way back — the bound is read from `openedNarrow`, which nothing after mount writes |
| **G1** no traffic, no socket change from a resize | **PASS.** `fetch` is stubbed to throw and is called **0** times; the `socket.on` count is flat across both resizes, and the rows survive — a rebuilt `FeedBuffer` would have re-registered the listener and dropped the ring. The buffer is keyed on `capacity`, which the resize does not move |
| **G2** success → Escape → login | **PASS.** The done panel survives Escape and `location.pathname` is `/login` |
| **G2** success → **Sign in again** | **PASS.** Same destination through the button |
| **G2** no accidental request before the navigation | **PASS.** The last request is `POST /api/v1/auth/password`, and the recorded list is byte-identical after the navigation. Nothing re-reads under a revoked cookie |
| **G3** two `GET /config` at once, A first, A settles last | **PASS.** A leaves at `t=1000` unanswered; `config_changed` at `t=2000` starts B rather than joining A (**two** in flight, which is the state the conditional join created); B answers with `ttl_seconds = 42`, then A answers with `10`. The form reads **42** |
| **G4** over-accounted row: chart gap, KPI state, the value after it | **PASS.** The three component bands are `null` and RSS is kept, so the chart's series and the rows carry the same readings — `rssStates(top)` equals `rssStates(rows)`, and the sample after the over-accounted one is judged against the state before it |
| **G4** the U1 carry is not broken again | **PASS.** A reading 1 MiB under the watch point stays `watch` inside the 3 MiB band, across an over-accounted row and across a row with no reading at all (`['watch', null, 'watch']`), where the same value alone reads `normal` |
| **G5** every group in the route table draws correctly | **PASS.** Driven off `ROUTES`, not off a name: each group's label appears and its children are exactly its member routes, in table order |
| **G5** a new group with no label fails to compile | **PASS, measured.** `RouteGroup` widened to `'diagnostics' \| 'ops'`: `routes.ts(44,14): error TS2741: Property 'ops' is missing in type '{ diagnostics: string; }' but required in type 'Record<RouteGroup, string>'`. That was the **only** error — the sidebar names no group any more, so nothing else had to be found by hand. Reverted |
| **G5** sidebar and top bar read one source | **PASS.** The sidebar's group line is `GROUP_LABELS[group]`, and the top bar renders `` `${GROUP_LABELS.diagnostics} · Memory` `` from the same record the shell passes it |

**Pre-fix proof, this pass.** G3's test was re-run with the `startedAt <
adoptedAt` guard removed: `expected '10' to be '42'` — the stale document won,
which is the race. G4's U1 test was re-run with the RSS gap restored:
`expected [108857600, null, 103857600] to deeply equal [108857600, 103857600,
103857600]` — the chart's walk and the card's diverge on exactly that row. G1's
and G2's cases were proved against the pre-fix code in §10.4. G5's two cases
pin the generic block; its regression is the compile error above, not a runtime
failure.

**§9.3's "no test for N14" is superseded.** It read that driving the race needs
a `SocketManager` emit seam that does not exist. `socket.on` is spied in
`live-feed.test.tsx` already, and the same spy captures the `config_changed`
listener here — with `Date.now` held still so the two reads carry distinct
instants. The seam was there.

#### Gates

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | 54 files, **923 passed**, 0 failed (+8) |
| `npm run build` | **128,363 B gzip** — unchanged; the additions are tests |
| `git status -- crates/` | empty |

#### Verdict

**PASS.** All twelve checks hold. Open, unchanged and non-blocking: **N5**,
**N6**, **N8**, **N9**, **N12**, **N16**, **N21**–**N24**, **T2**–**T4**, **M2**.
