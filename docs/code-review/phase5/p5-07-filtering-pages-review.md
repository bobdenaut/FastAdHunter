# p5-07 — Filtering Pages · Review

**Task:** [p5-07-filtering-pages.md](../../../plan/wip/phase5/p5-07-filtering-pages.md) ·
**Plan:** [p5-07-filtering-pages-plan.md](../../../plan/wip/phase5/p5-07-filtering-pages-plan.md) ·
**Branch:** `phase5-07` · **Depends on:** `p5-06`

---

## Implementation Summary

### What was implemented

Four shipped routes — `/rules`, `/policies`, `/clients`, `/rule-tester` — all
`built: true`, all declaring `events: []` and `endpoints: []`. The task also
lands the two pure modules the rest of the phase inherits: `policy/selectors.ts`
(the client-selector matcher) and `policy/assignment.ts` (the direct-versus-
inherited classification), plus `policy/validation.ts` (the config-validator
mirrors and the `422` parser).

Work followed the plan's units **W1 → W14**, each ending with `npm run
typecheck`, `npm run test` and `npm run build` green before the next.

### Files and modules

| Area | Files |
| ---- | ----- |
| API client (new) | `src/api/{policies,rules}.ts`, `src/api/filtering.test.ts` |
| API client (changed) | `src/api/clients.ts` (+3 mutations), `src/api/types.ts` (+11 shapes), `src/api/index.ts` |
| Pure modules (new) | `src/policy/{selectors,assignment,validation}.ts` + three test files |
| Components (new) | `src/components/{policy-chip,line-editor,busy-modal}.tsx` + `line-editor.test.ts` |
| Components (changed) | `src/components/data-age.tsx` (re-exports `formatAge`), `src/time.ts` (`formatAge` moved here, `lastSeenLabel` added) |
| Custom Rules | `src/pages/rules.tsx` + `src/pages/rules/{error-list,how-this-saves}.tsx` + `rules.test.tsx` |
| Policies | `src/pages/policies.tsx` + `src/pages/policies/{policy-card,default-card,policy-dialog,assignment-rows,cost-card}.tsx` + `policies.test.tsx` |
| Clients | `src/pages/clients.tsx` + `src/pages/clients/{client-row,assign-dialog,rename-field,legend-card}.tsx` + `clients.test.tsx` |
| Rule Tester | `src/pages/rule-tester.tsx` + `src/pages/rule-tester/{query-form,result-card,session-ring}.tsx` + `rule-tester.test.tsx` |
| Shell seam | `src/router/routes.ts` (`ownsHeader`), `src/shell/shell.tsx`, `src/shell/content-header.tsx` (+`actions`), `src/shell/content-header.test.tsx` |
| Router | `src/router/router.ts` — `blockNavigation()` / `navigationBlocked()` |
| Invariants | `src/pages/filtering-invariants.test.ts` (new), `src/styles/grid-tracks.test.ts` (extended) |
| Styles / gallery | `src/styles/components.css`, `src/pages/dev-gallery.tsx` (policy-chip specimens) |

**No Rust source changed** — `git status -- crates/` is empty. No new API route,
no new config key, no new dependency. *(True at implementation time; the
approved F1 fix later reordered `put_user_rules` in `routes.rs` and added one
test in `api.rs` — see §Fixes applied.)*

### Important design decisions

**The header seam is a route declaration, not page state.** `Route.ownsHeader`
makes the shell skip both its `<ContentHeader>` **and** its `<main class="wrap">`
for that route, because the two are siblings and `.hd` sits outside the padded
content column. While the lazy chunk is still resolving the shell renders
neither, so entering `/rules` cannot flash `Custom Rules` before the page
replaces it with the artboard's `Custom rules`. Lists is **not** retrofitted
(§15).

**Recompiling mutations are never aborted, and the block is in the router.**
`blockNavigation()` is a refcounted, idempotent hold that `navigate()` consults.
Held across `POST /policies`, `DELETE /policies/{id}`, a `PATCH` that changes
`lists`, and `PUT /rules/user`; none of the four carries an `AbortSignal`. The
`BusyModal` is a new shared component — no cancel, no faked progress, one
focusable element so the trap has somewhere to put focus.

**The Clients table is one grid, via `subgrid`.** The header and every row are
`grid-column: 1 / -1; grid-template-columns: subgrid` children of one
`.clients-table` definition, so they cannot drift the way `p5-06`'s F1 pair did.
Two subgrid traps were found by measurement and are commented in the CSS:

- a subgrid item's own **padding** shortens the inherited tracks — the 44 px
  action track arrived **25 px** wide at 1400 px. The padding moved to the
  parent.
- a subgrid that declares its own **`gap`** overrides the parent's and takes the
  gutter out of the inherited tracks — 70 px came off the eight columns and the
  track arrived **39 px**. `gap` is now declared on the parent only.

  After both, header and rows measure byte-identical lefts and widths, and the
  glyph is 44 × 44.

**`formatAge` moved from `components/data-age.tsx` to `time.ts`** and is
re-exported from its old home. Clients needs it for `last_seen` and a formatting
helper importing a component module is backwards layering. One day-form branch
was added (`9 d ago`); it is unreachable by a poll age.

**Nothing on these four pages holds a clock.** Every "in force" / "window shut"
statement is read off `client.policy`. `filtering-invariants.test.ts` asserts by
source read that `src/policy/` contains no `Date`, `toLocale` or
`getTimezoneOffset`, that no page imports a chart module or `useRefresh`, and
that none listens to a viewport.

**Two shared components were added that the plan's file list does not name**,
because the alternative was duplication: `components/busy-modal.tsx` (Custom
Rules and Policies both need the same blocking state) and the `PolicyChip`
already planned. Conversely `pages/rules/editor-gutter.tsx` was **not** created
— the gutter is three elements inside `LineEditor` and splitting it would add a
file without adding a seam.

### Deviations from the artboards

The plan's registry X1–X5 is implemented as written. Four further departures,
all declared here:

| # | Artboard | Shipped | Why |
| - | -------- | ------- | --- |
| new | `Clients.dc.html` draws the table footnote's schedule as `mon–fri 21:00 → 07:00` | `mon–fri · 21:00 → 07:00` | The plan's §7.4 fixes one formatter for three pages; its table uses `·` between the days and the window. |
| new | `Clients.dc.html`'s **Editing a client** card draws a form specimen | the card carries the artboard's explanatory sentences; the live editor is the row's expanded region and the assign dialog | A second, non-functional copy of the editor is dead UI. |
| new | `MobileClients.dc.html` draws one stats line (count · bar · %) | two lines: `queries` / `blocked` on one, bar + % beneath | The desktop table has a `Blocked` column the artboard's phone card drops; keeping it costs a line and keeps one DOM. |
| new | `CustomRules.dc.html` draws the Precedence badges as solid green/red/grey squares with white numerals | the pill token pairs (`--pill-good-*`, `--pill-bad-*`, `--pill-neutral-*`) | `tokens.css` forbids a literal colour outside itself and `literal-colours.test.ts` enforces it; white-on-solid needed a new token for three badges. |

### Which callout treatment shipped (§7.1)

**The primary one.** The floating callout renders immediately below the bad
line, overlaying the line beneath it; the fallback (collapse to a marker) was
not needed. Measured at 390 px with a ten-line document and one bad line: the
callout is `white-space: nowrap` with `text-overflow: ellipsis` inside the
overlay, the page body does not scroll sideways, and the anchored error list
under the editor exists regardless — each entry is a ≥ 44 px button that focuses
the textarea and selects that line's range.

### Tests — what they demonstrate

Counts are not the point; these are the capabilities the suite pins, and what it
cannot reach.

| Capability | Demonstrated by | Not covered by tests |
| ---------- | --------------- | -------------------- |
| **R1's branch order** — a name assignment is tested before a shut window | `assignment.test.ts`: an open direct window overridden by a schedule-less name assignment yields 2a and asserts the note does **not** contain `window shut` | — (also staged live, V7a) |
| **2a′ refuses attribution** when every name member is scheduled | `assignment.test.ts` asserts the note names neither the name assignment nor a shut window | — |
| **Branch 0, both shapes** — the two responses disagree | `assignment.test.ts`: `assignment_source` present with no string-equal assignment → no note; absent with one → branches 3/4 | — |
| **The direct lookup is string equality** | `assignment.test.ts` pins `"192.168.010.5"` not matching `172…10.5`, and first-in-order selection | — |
| **The selector matcher matches the engine's** | `selectors.test.ts`: leading-zero rejection, `/0` `/32` `/128`, mixed families never match, a malformed address before `/` is not a name, ASCII fold, unnamed never matches a `Name` | Ported from the Rust's rules by reading; not executed against the Rust |
| **Clients issues exactly two requests, never a per-row policy read** | `clients.test.tsx` asserts the call list is `[GET /clients, GET /policies]` and that no `GET …/policy` appears | Request count at scale — staged live at 25 clients (V4) |
| **Direct/inherited semantics** | `clients.test.tsx` renders all five branches the fixture reaches and asserts chip style, dashedness and note text | — |
| **Clients mutations are live** | `clients.test.tsx`: rename / assign / clear each raise **no dialog**, and each is followed by both re-reads in the call list; `404` on clear is silent, `404` on assign is reported | Wall-clock timing — measured live (V8a) |
| **The recompile boundary** | `policies.test.tsx`: a rename sends `{name}` only with no confirm and no modal; an assignments-only edit sends no `lists`; a `lists` change confirms first (0 requests before the confirm), then blocks, blocks `navigate()`, and sends `{"lists": null}` | — |
| **Form-enforced limits** | `policies.test.tsx`: `default` refused, the alphabet message, the `409` rendered with the id kept, the ceiling disabling both `New policy` buttons | — |
| **Custom Rules anchoring and text preservation** | `rules.test.tsx`: a `422` anchors both lines, the buffer is compared as a **string** and is byte-identical (trailing blank line included) after `422` **and** after `500`; an unparseable message yields zero anchors and zero callouts | — |
| **The blocking save** | `rules.test.tsx`: modal up, editor + both buttons disabled, `navigate()` refused, a second `PUT` impossible, block released on failure too | — |
| **The 422 parser is total** | `validation.test.ts`: a quoted rule containing `; `, the `and N more` tail, the 100-line cap, an unparseable message → zero anchors | — |
| **Rule Tester name resolution** | `rule-tester.test.tsx`: `TV` finds `tv` and the address is substituted **visibly**; two matches block with **zero** requests sent; an unobserved name renders the partial banner and the "why" claims nothing | — |
| **The session ring is bounded** | `session-ring.ts`'s `pushRecord` is pure; 40 pushes leave 10, newest first | — |
| **The four routes reach for nothing** | `filtering-invariants.test.ts`: no chart import, no `useRefresh`/`RefreshCluster`/`socket.on`, no scheduler, no viewport listener, no date arithmetic in `src/policy/`, no `getClientPolicy` accessor | An absence proven by source read, not by architecture |
| **The Clients grid** | `grid-tracks.test.ts`: eight columns, a fixed 44 px last track, `subgrid` + `1 / -1` on both header and rows, the scroller on the table | jsdom resolves no layout — the widths were measured in a browser (V14/V15) |

`npm run test` — **500 vitest cases in 40 files** (p5-06 shipped 272 in 23).

### Measurements

Chromium via Playwright against `fastadhunter:p5-05` in Docker on the dev box
(`fah-p506`, published on 18443), the Vite dev server proxying to it. Dev-box
figures for a bundle and a browser; nothing here is an RB5009 measurement.

**Bundle — gzip gates, brotli is what travels:**

| file | raw | gzip | brotli |
| ---- | ---: | ---: | ---: |
| `assets/uPlot.esm-*.js` | 50,996 | 21,997 | 19,884 |
| `assets/style-*.css` | 38,815 | 8,293 | 7,385 |
| `assets/index-*.js` | 26,336 | 8,561 | 7,638 |
| `assets/dashboard-*.js` | 23,630 | 8,666 | 7,789 |
| `assets/lists-*.js` | 15,085 | 4,823 | 4,246 |
| `assets/policies-*.js` | 14,342 | 4,703 | 4,168 |
| `assets/jsxRuntime.module-*.js` | 10,735 | 4,496 | 4,114 |
| `assets/clients-*.js` | 10,673 | 3,817 | 3,354 |
| `assets/rule-tester-*.js` | 9,681 | 3,429 | 2,993 |
| `assets/rules-*.js` (page) | 7,248 | 2,914 | 2,525 |
| `assets/sprite-*.svg` | 5,501 | 1,185 | 1,034 |
| `assets/assignment-*.js` | 5,259 | 2,040 | 1,835 |
| `assets/icon-*.js` | 4,257 | 1,951 | 1,735 |
| `assets/validation-*.js` | 2,065 | 1,036 | 892 |
| `assets/login-*.js` | 1,929 | 975 | 838 |
| `assets/time-*.js` | 1,282 | 628 | 567 |
| `index.html` | 1,130 | 609 | 433 |
| `assets/stage-bar-*.js` | 868 | 476 | 411 |
| `assets/confirm-dialog-*.js` | 663 | 367 | 339 |
| `assets/busy-modal-*.js` | 504 | 314 | 265 |
| `assets/card-*.js` | 450 | 275 | 218 |
| `assets/rules-*.js` (api) | 339 | 207 | 178 |
| `favicon.svg` | 258 | 185 | 163 |
| `assets/figure-*.js` | 198 | 164 | 125 |
| `assets/system-*.js` | 180 | 158 | 129 |
| `assets/stats-*.js` | 132 | 138 | 114 |
| **TOTAL** | **232,556** | **82,407** | **73,372** |

**82,407 B gzip against the 153,600 B budget — 53.7 %.** Brotli 73,372 B.
p5-06 shipped 59,490 B gzip; four pages added 22,917 B.

**Mutation timing — dev box, 60,420-rule ruleset, compile 0.022 s:**

| Operation | Recompiles | Measured |
| --------- | ---------- | -------- |
| `PUT /rules/user` | yes | **33 ms** |
| `PATCH /policies/{id}` changing `lists` | yes | **26 ms** |
| `DELETE /policies/{id}` | yes | **35 ms** |
| `PATCH /policies/{id}` changing `name` | no | **13 ms** |
| `PUT /clients/{ip}` (rename) | no | **12 ms** |
| `PUT /clients/{ip}/policy` | no | **11 ms** |

**The millisecond-versus-second contrast the plan expects is not reproducible on
this fixture** and the figures above should not be read as it. This container
serves one small list (60,420 rules, 22 ms compile); the Lists header's
`7.41 s · last compile` is a full-corpus figure on the RB5009. What the figures
do establish is the **ratio**: a recompiling mutation costs ~2–3× a live one
even at this corpus size, and the ratio grows with the ruleset. The UI treatment
does not depend on the figure — it is chosen from the handler's `Recompile`
value, not from a duration.

**The Clients table's own minimum width: 882 px**, needing a **1004 px**
viewport at the 768–1199 px breakpoint (122 px of chrome). It therefore fits
inside the 1200 px full-grid contract with 196 px to spare — unlike Lists, which
needed 1247 px.

### Verification

Read off a live browser, a request log, and the API container's own network
namespace. Method notes are given where a check could not be produced.

| # | Result |
| - | ------ |
| **V1** | **PASS.** A `422` on a ten-line document anchored `line 7` in the callout, the error list and the banner. The buffer was compared **as a string** before and after: identical, trailing `'   '` line included. |
| **V1a** | **PASS.** `GET /rules/user` after the failed `PUT` returned `{"rules":[]}` — the pre-edit document, unwritten. |
| **V1b** | **PASS.** With the envelope stubbed to `the rules document was rejected by the compiler`: **zero** anchors, **zero** callouts, the raw text in the banner, no fabricated line number, buffer unchanged, and the secondary dropped the `invalid` count. |
| **V1c** | **PASS.** A ten-line document with one exact duplicate came back as nine; the page reported `Saved. 1 duplicate line removed` and rendered the returned document. |
| **V2** | **PASS.** `fahUnion()` `[]`, `fahSocketState()` `closed`, `fahTimers()` `0` on all four routes. |
| **V2a** | **PASS — server-side.** Counted **inside the API container's own network namespace** (`docker run --rm --network container:fah-p506 busybox netstat -tn`), which is a stronger vantage than the plan's Docker-host fallback. Parked on each route in turn: `/rules` **0**, `/policies` **0**, `/clients` **0**, `/rule-tester` **0** ESTABLISHED to `:8443`. Parked on the Dashboard for contrast: **1**. |
| **V2b** | **PASS.** `registry.activeTimers()` read **0** on each of the four; `REFRESH_ENDPOINTS` is still exactly `['health','telemetry','cache','clients','lists']`, pinned by `routes.test.ts`. |
| **V2c** | **PASS in part.** The source grep is `lifecycle/timers.test.ts` (unchanged, still green) plus `filtering-invariants.test.ts`, which additionally asserts none of the four pages or `src/policy/` names `subscribeAgeTick`, `every(` or `after(`. `ageTickerRunning()` is **not** exposed to the browser, so the runtime half was not read off the DOM; nothing on these pages can start it, since `DataAge` is not imported by any of them. |
| **V3** | **PASS.** Dashboard → each of the four → Dashboard. On every one of the four: union `[]`, socket `closed`, timers `0`. On return: union `['stats']`, socket `open`, timers `5`. |
| **V4** | **PASS.** Fresh load of `/clients` against an inventory of **25 observed clients** issued exactly two API requests — `GET /api/v1/clients`, `GET /api/v1/policies` — read off the request log, not inspected. The inventory was staged by running 24 concurrent containers on the bridge, each sending one query. |
| **V5** | **PASS.** The whole page session's request log filtered for `/clients/…/policy` holds exactly two entries, both the page's own writes: `PUT …/172.17.0.4/policy` and `DELETE …/172.17.0.4/policy`. **Zero `GET`.** |
| **V6** | **PASS.** Staged for real through the API. Direct with a schedule → solid `kids`, `daily · 20:00 → 22:00 · window shut now — default in force`. Subnet-inherited → dashed `guest`, `via 172.17.0.8/32`. Name-inherited → dashed `guest`, `via name printer`. Unassigned → dashed `default`, `inherited · no assignment`. |
| **V6a** | **PASS.** With `guest` holding both `172.17.0.10/32` and the name `tablet`, and `172.17.0.10` named `tablet`, the row rendered dashed `guest` and the bare word `inherited` — no selector named. |
| **V7** | **PASS.** See V6's first row: `GET /clients` reported `policy: "default"` with `assignment_source: "direct"` for `172.17.0.5`, and the row showed `kids` solid, marked shut, naming `default` as what is in force (X4). |
| **V7a** | **PASS — R1 staged.** `172.17.0.11` carried a direct `kids` assignment with an **open** window (13:00–16:00, tested at 14:5x UTC) and a **schedule-less** name assignment on `guest`. `GET /clients` reported `guest`; the row rendered 2a's sentence — `not in force — the name assignment on "console" decides (guest)` — and **not** `window shut now`. |
| **V7b** | **PASS.** The same shape with the `console` name assignment **scheduled** rendered `not in force — guest in force`: no name claim, no shut-window claim. |
| **V8** | **PASS.** A `lists` change raised `Change which lists this policy holds?` naming `seconds of CPU on the router`, and **zero** requests were sent before the confirm. After confirming: the busy modal `Rebuilding the ruleset` was up, `navigate('/clients')` left the path on `/policies`, and the modal cleared on the response. A delete raised `Delete filler-1?` naming what goes with it, blocked identically, and the ceiling figure moved `16 / 16 → 15 / 16`. |
| **V8a** | **PASS.** Three live mutations, no dialog and no busy modal on any: policy rename **13 ms**, client rename **12 ms**, client assign **11 ms** — measured from `PerformanceResourceTiming`, not estimated. Against the recompiling `lists` change at **26 ms**. See the caveat above the timing table. |
| **V8b** | **PASS.** `PUT /rules/user` measured **33 ms**, the same order as the `lists` change. During it: modal naming the rebuild, editor + `Discard` + `Validate and save` all disabled, navigation blocked, no abort, no second `PUT` startable, and **no optimistic "Saved."** before the response. |
| **V9** | **PASS.** The Default card renders `reserved` with **zero** buttons. `default` in the create form gives `"default" names the implicit policy…` and disables `Create policy`. The API's own `422` message is what the form mirrors, and the real `409` on a duplicate id rendered `policy kids already exists` with the id kept **and focused**. |
| **V10** | **PASS.** With 15 configured policies: summary `16 / 16`, slot card `0 policy slots left` plus the reason, and **both** `New policy` buttons disabled with the ceiling named in the title. The API refused a 16th with `422 at most 16 policies may exist including the default; got 17`. |
| **V11** | **PASS in part.** `active_assignments` is rendered verbatim and observed at three different values across staging steps (0 → 6 → 2) as assignments were added and name selectors expanded. The timezone renders truncated (`EET-2EEST`) with the full POSIX string `EET-2EEST,M3.5.0/3,M10.5.0/4` as `title`. **No date arithmetic exists in `src/policy/`** — asserted by source grep in `filtering-invariants.test.ts`. **Not produced:** a change observed across a real schedule boundary with no page action other than re-entry; the values above moved because the configuration moved. |
| **V11a** | **PASS.** With `guest` holding the single name assignment `tv` and two observed clients named `tv` / `TV`, the API reported `active_assignments: 2` against **3** configured assignment rows. The UI printed `2 · assignments in force right now` and `3 · assignments configured`, and the page text contains no `of N configured` phrasing. |
| **V12** | **PASS.** Client mode, address: a block returned `||analytics.google.com^` / `oisd-basic` / `guest` plus the derived `via name printer`; a pass returned both fields null, rendered `no rule matched` and `—`, with `no assignment covers this address`. `user-rules` renders as **your custom rules** (verified against a rule this page itself saved). |
| **V12a** | **PASS.** Policy mode: `kids` and `default` both returned all four fields; the "why" read `you chose this policy — assignments are ignored`; the request carried `policy` and no `client`. |
| **V12b** | **PASS.** One match: `CONSOLE` resolved to `172.17.0.11`, the card printed `tested as 172.17.0.11 (console)`, and **deciding policy read `guest`** — the value a raw name could never produce. Two matches: the prompt listed `172.17.0.5 tv` and `172.17.0.6 TV`, **zero** `POST /rules/test` were sent, and choosing one sent that address. No match: the partial banner rendered and the "why" read `no address was given, so no assignment could apply`. |
| **V13** | **PASS.** Every rendered figure traces to a field or to a plan §8.5 `T` row — the table below. No derivation exists that §8.5 does not list. No per-assignment in-force claim anywhere: `ACTIVE NOW` and the per-assignment dot are absent from the DOM. |
| **V14** | **PASS.** Light theme at **1400 / 1247 / 1200 / 900 / 390 px** and dark theme at **1400 / 390 px**: `scrollWidth − clientWidth` is **0** on both `documentElement` and `.main` for all four routes at every width. The Clients table scrolls inside its own container only where it must — 0 px at 1400/1247/1200/390, **104 px at 900 px**. Its own minimum is 882 px (1004 px viewport). The dark pass was run at the two extremes rather than all five: no sizing rule in this task is theme-conditional. |
| **V15** | **PASS with two pre-existing exceptions.** At 390 px, measured from the DOM across all four routes, the only controls under 44 px on either axis are the drawer footer's **Theme (34 × 44)** and **Sign out (42 × 44)** — `p5-05` shell controls inside the drawer, which `p5-06`'s V17 already excluded and which this task does not touch. Two of this task's own controls failed first and were fixed: the search box (34 px tall at 390) and the `A` query-type chip (38 px wide). |
| **V16** | **PASS.** At 390 px Clients is one card per client, header hidden, laid out in `MobileClients.dc.html`'s order — name, address, chip row, stats, last seen, glyph top-right. Tapping expands `Rename` and `Change policy` side by side at 145 × 44 each **in place**; `location.pathname` is unchanged, so no screen is pushed. The shut window is called out in words and takes the warn foreground. |
| **V17** | **PASS.** Custom Rules at 390 px keeps a 34 px gutter and its anchoring, the two side cards stack under the editor, and the editor scrolls inside itself. Policies stacks one card per row (349 px each) with assignment rows intact (2 and 5), the summary goes two-up and `What costs what` stacks. The Rule Tester result card sits **16 px** beneath the form, and the `kv` grid is single-column. |
| **V18** | **PASS.** Figures above. Four new lazy route chunks (`rules`, `policies`, `clients`, `rule-tester`). **No chunk from this task references `uplot`** — asserted by grepping the built assets: only `dashboard-*.js` and `uPlot.esm-*.js` do. The single CSS `uplot` hit is `p5-06`'s hand-written `.chart .uplot` scope (m8 option (c)). |
| **V19** | **PASS in part.** The structural half is proven: `activeTimers()` 0, union `[]`, socket `closed` after five rounds over all four pages, and the Rule Tester ring stayed at **10 rows after 40 tests**. **The heap half was NOT produced.** Across five rounds `usedJSHeapSize` rose 22.4 → 34.1 MiB with one collection observed at the end (31.6 MiB); a second attempt using allocation pressure to provoke a collection inflated the reading instead (127 → 266 MiB) because the pressure buffers are themselves counted. Without a forced GC (`--expose-gc`, or CDP `HeapProfiler.collectGarbage`) these numbers are evidence in neither direction and are recorded as such. |
| **V20** | **PASS.** `cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --all-features --workspace` **1,195 passed, 0 failed** — unchanged from p5-06, as expected for a task that touched no Rust (`git status -- crates/` is empty). Frontend: `npm run typecheck` clean, `npm run test` **500 passed**, `npm run build` clean at 53.7 % of budget. |

### Every rendered figure, and where it comes from (V13)

| Display | Source |
| ------- | ------ |
| `10 lines` | T1 — `rules.length` |
| `1 invalid` | T2 — distinct anchored lines + the stated `and N more` remainder |
| `1 duplicate line removed` | T3 — `sent.length − response.rules.length`, rendered only when > 0 |
| `3 / 16` | T4 — `items.length + 1` over the constant 16 |
| `13 policy slots left` | T5 — `16 − (items.length + 1)` |
| `3 assignments configured` | T6 — `Σ items[].assignments.length` |
| `EET-2EEST` (+ full string as `title`) | T7 — `timezone` up to the first `,` |
| policy traffic bar | T8 — `blocked / queries` from `/stats.policies`, empty track at `queries === 0` |
| client blocked-share bar and `%` | T9 — `blocked_24h / queries_24h × 100`, the bar's width **is** the percentage |
| `unnamed` | T10 — `name === null` |
| `2 m ago` / `6 h ago` | T11 — `last_seen` against the render's own `now`, no ticker |
| chip style and note | T12 — §7.3 over `/clients` + `/policies` |
| `why that policy` | T13 — §8.4 over the same two |
| schedule text | T14 — §7.4 formatting of `days` / `start` / `end` |
| `2 assignments in force right now` | `active_assignments`, verbatim |
| `60,425 rules` in What-costs-what | `compiled_rules` from `GET /lists`, verbatim |
| verdict, rule, list, policy | the four `RuleTestResponse` fields, verbatim |
| everything else | read from a field verbatim |

### Known limitations and deferred items

1. **V19's heap half was not produced**, and V11's "changes across a real
   schedule boundary" half was not staged. Both stated per row above.
2. **The mutation timings are not the plan's expected contrast.** This fixture
   compiles in 22 ms; the ratio holds, the absolute figures do not transfer.
   An RB5009 measurement belongs to `p5-10`.
3. **A recompiling request survives a dropped browser, not a dropped tab.** The
   modal and the router block prevent an in-app unmount; a tab close or a Wi-Fi
   loss mid-compile still drops the connection and Axum still cancels the
   handler future. That is a server-side exposure predating this task, recorded
   here rather than fixed.
4. **Two shell observations found during verification, neither this task's.**
   With the phone drawer open, the page body behind it still scrolls (no scroll
   lock), and `.sb-nav` takes its own vertical scrollbar (694 px of links in a
   662 px box at a 844 px viewport). Both are `shell/sidebar.tsx` /
   `layout.css`, untouched here. Neither is a horizontal overflow, so V14 is
   unaffected.
5. **`subgrid` is now load-bearing** for the Clients table. It has no fallback:
   a browser without it would render the rows as single-column. The application
   already requires `:has()` and ES2022, so the floor was already modern.
6. **The dev fixture was left staged.** `fah-p506` now holds two policies
   (`kids`, `guest`) with the assignments the verification needed, five named
   clients, a nine-line user-rules document, and `schedule.timezone` set to
   `EET-2EEST,M3.5.0/3,M10.5.0/4` (it was `UTC`). Twenty-four probe containers
   were started and have exited.
7. **`ageTickerRunning()` is not exposed to the browser**, so V2c's runtime half
   rests on the source assertion plus the fact that `DataAge` — the only caller
   — is not imported by any of the four pages.

### Documentation

**No repository document was changed.** The plan's §13 proposes nine edits —
three to `API.md`, five to `docs/dashboard/sketch/*`, one to
`information-architecture.md`. None is applied; they await the owner's yes and
are listed for that decision in the hand-off, together with the four additional
artboard deviations declared above.

---

## Findings

Adversarial review of the full p5-06 → HEAD range (all 50 staged frontend
files), 2026-08-27. Ground truth read directly: `routes.rs`, `wire.rs`,
`fah-model/src/policy.rs`, `fah-rules/src/policy.rs`,
`fah-config/{lib,schema/policy}.rs`, API.md, and the five artboards. Key
measurements reproduced live against `fah-p506` through the Vite dev server —
not taken from the sections above.

### Re-verified independently — claims that held

| Claim | Result |
| ----- | ------ |
| Gates | `cargo fmt` / `clippy -D warnings` clean; `cargo test` **1,195 passed, 0 failed**; `npm run typecheck` clean; `npm run test` **500/500**; build **82,407 B gzip (53.7 %)**, brotli 73,372 — byte-identical to the table above |
| V2/V2b | `/clients`: `fahUnion() []`, `fahSocketState() closed`, `fahTimers() 0` |
| V2a | parked on `/clients`: **0** ESTABLISHED to `:8443` inside the API container's own netns (`docker run --network container:fah-p506 busybox netstat -tn`) |
| V3 | return to Dashboard: union `['stats']`, socket `open`, timers `5` — p5-06 lifecycle intact |
| V4/V5 | fresh `/clients` load: exactly `GET /clients` + `GET /policies`, zero `GET …/policy` |
| V14 | 1400 / 900 / 390 px: `scrollWidth − clientWidth` = 0 on `documentElement` and `.main`, all four routes; Clients table scrolls only internally |
| V15 | only sub-44 px controls at 390 px: drawer `Theme` 34.5 × 44, `Sign out` 42.5 × 44 — the two pre-existing p5-05 exceptions, exactly as stated |
| V16 | card expands `Rename` / `Change policy` in place at ≥ 44 px; `location.pathname` unchanged; desktop header `display: none` |
| X2 | `ACTIVE NOW` absent from the live Policies DOM |
| Selector mirror | `selectors.ts` checked line-for-line against `fah-model/src/policy.rs:127-181` and `fah-rules/src/policy.rs:460-481`; `assignment.ts` branch order against §7.3; both faithful |
| Rust untouched | `git status -- crates/` empty; staged set is p5-07 paths only; the modified `phase2.6` review file is left unstaged per plan §2 |

### F1 — Major — FIXED (2026-08-27) — 422 anchors point at the wrong line when a duplicate precedes an invalid line

`put_user_rules` **dedups before it validates** (`routes.rs:1188-1197`, then
`validate_user_rules` at `:1205`), so the `line N` in the 422 message indexes
the **deduped** document. `rules.tsx:106` anchors those numbers onto the buffer
**as typed**, which still contains the duplicates — every anchor below a
dropped duplicate is off by the number of duplicates above it.

**Staged live** (buffer preserved, nothing written): typed
`||dup.example^` / `||dup.example^` / `@@||^` → API answered
`line 2: invalid rule syntax: "@@||^"` → the UI anchored, called out and
band-highlighted **line 2, whose content is the valid `||dup.example^`**; the
real invalid line 3 carries nothing.

- Violates the acceptance criterion "anchors every message to the right line";
  V1's PASS was produced on a duplicate-free document and does not generalize —
  the dup + invalid interaction is tested nowhere (`rules.test.tsx` covers each
  separately).
- Impact: the error list's click selects a valid line; the operator "fixes" the
  wrong rule and the save fails again.
- **Measured. Fix before DONE.** Smallest fix: map server line numbers back to
  buffer lines through a client-side mirror of the dedup (trim; lines starting
  `#`/`!` and blanks exempt; keep-first) before anchoring.

### F2 — Major — FIXED (2026-08-27) — browser Back/Forward bypasses the recompile navigation block

`onPopState` (`router.ts:85-87`) announces unconditionally; only `navigate()`
consults `navigationBlocks`. **Staged live:** with the busy modal up and an
in-flight (stubbed-pending) `PUT /rules/user`, `history.back()` moved the route
to `/clients` — modal gone, editor unmounted, block still held.

- Violates R2 / plan §10.2: "the busy state is a modal that **also blocks
  in-app navigation**: unmount cannot happen while a recompiling mutation is in
  flight". V8/V8b's PASS tested `navigate()` only.
- Impact: the operator loses the only indication a compile is running and can
  start a second recompiling mutation from a re-entered page. The request
  itself is **not** aborted (none carries a signal), so the persist/swap race
  stays closed — the exposure is the UI contract, not data loss.
- **Measured. Fix before DONE.** Smallest fix: in `onPopState`, when
  `navigationBlocked()`, push the current path back
  (`history.pushState(null, '', currentPath())`) and skip the announce.

### F3 — Major — FIXED (2026-08-27) — an empty `lists` subset renders as "every enabled list", the inverse of what the engine does

`ListChips` (`policy-card.tsx:41`) folds `lists.length === 0` into the
`lists === null` branch. The engine does the opposite: `mask_for_list`
(`fah-rules/src/policy.rs:211-215`) gives `None` every list and `Some([])`
**none** — a policy with an empty subset blocks nothing.

**Staged live:** `POST /policies {id: "empty-subset-probe", lists: []}` → 201
echoing `"lists": []` → the card's chip read **"every enabled list"**. (Probe
deleted, 204.)

- The state is reachable through this page's own dialog ("only these" with
  nothing ticked → `patch.lists = []`).
- Impact: the card claims full protection for a policy that filters nothing.
- **Measured. Fix before DONE.** Smallest fix: render `[]` as its own state
  ("no lists — blocks nothing"), and preferably refuse an empty subset in the
  dialog the way an empty id is refused.

### F4 — Should-fix — FIXED (2026-08-27) — a Clients mutation completing after unmount fires both re-reads from a dead page

`run()` (`clients.tsx:114`) chains `.then(() => load())` unconditionally, and
the mutations deliberately carry no signal — so a rename answered just after
navigation issues a fresh `GET /clients` + `GET /policies` attributable to the
unmounted route, with a controller nothing will ever abort. Violates the phase
invariant "leaving a page stops its work" (V3's criterion) in the
mutate-then-navigate window. Inferred from code, not staged. Fix: a `disposed`
flag set in the effect cleanup, checked before `load()`.

### F5 — Minor — FIXED (2026-08-27) — concurrent mutations on two Clients rows clobber each other's UI state

Buttons are disabled per-row (`busy={busyIp === client.ip}`), so a second
row's mutation can start while the first is in flight; the first `finally`
(`clients.tsx:115-119`) then sets `busyIp`/`renamingIp`/`assigningIp` to `null`
unconditionally, clearing the second row's busy indicator and closing its open
rename/assign editor mid-flight. Millisecond window, no data corruption.
Inferred. Fix: functional updates that clear only when still equal to this
run's ip.

### F6 — Minor — FIXED (2026-08-27) — a failed post-mutation re-read is invisible

`clients.tsx` renders `loadError` only while `clients === null`; after a
successful first load, a failed re-read leaves stale rows and no message
(`policies.tsx` `reloadPolicies` has the same shape). The next entry re-reads,
but the instant after a mutation is exactly when the operator is looking.
Inferred. Fix: route reload failures into the mutation-error slot.

### F7 — Minor — FIXED (2026-08-27) — the Rule Tester POST is not aborted at unmount, contrary to plan §5.4

Plan §5.4: "unmount → in-flight aborted." `send()` calls `testRule(body)` with
no signal (`rule-tester.tsx:106`); only the two entry GETs are aborted. The
late response merely no-op-setStates, but the declared abort discipline is not
implemented for the POST. Inferred. Fix: thread the entry controller's signal
and swallow `AbortError` — or amend §5.4.

### F8 — Minor — FIXED (2026-08-27) — the `N lines` figure is derived from the buffer, not T1's `rules.length`; an empty document reads "1 line"

`rules.tsx:118`: `buffer.split('\n').length` once a document is loaded — an
empty document (`rules: []`, a fresh install) renders **"1 line"**. This is
also a derivation §8.5 does not list (T1 says `rules.length`), which V13
asserts cannot exist. Fix: treat an empty buffer as 0 lines and restate T1 as
"buffer line count" if the live count is intended.

### F9 — Minor — FIXED (2026-08-27) — both busy modals claim acceptance before the server has answered

"The document was accepted and the whole ruleset is being recompiled…"
(`rules.tsx:241`) shows from request start; validation happens **inside** that
request, and a 422 disproves the sentence the modal spent seconds asserting.
Same pattern in `policies.tsx:375`. R2 requires the busy state to "state what
is happening". Fix: "being validated and recompiled" wording.

### F10 — Nitpick — FIXED (2026-08-27) — two parse divergences from Rust in `selectors.ts`

- `parseV6` accepts an embedded v4 form in the *head* of a `::` address —
  `1.2.3.4::` parses here, while Rust requires the v4 form at the very end and
  rejects it, so the engine reads that selector as a `Name`.
- The prefix-length check `/^[0-9]+$/` rejects `192.168.1.0/+24`, which Rust's
  `u8::from_str` accepts.

Both need a pathological hand-edited TOML; the classification degrades to
`inherited`/silence, never a wrong claim. Pin with tests or align.

### F11 — Nitpick — FIXED (2026-08-27) — the anchor scanner can be fed a fabricated anchor

An invalid line whose own text contains `line 5: invalid rule syntax: ` puts
that pattern verbatim inside the `{content:?}` quotes, and the scanning parser
(`validation.ts:117`) mints a spurious line-5 anchor — the plan's "never
fabricates a line number" holds for format changes, not for hostile content.
Self-inflicted, bounded, and the alternative (parsing Rust debug-escapes) is
not worth it; record as an accepted limitation.

### Review-file corrections

| Section above | Correction |
| ------------- | ---------- |
| V1 | Its PASS holds only for duplicate-free documents — see F1. |
| V8 / V8b | "navigation blocked" was proven for `navigate()` only; popstate bypasses it — see F2. |
| V13 | "No derivation exists that §8.5 does not list" is contradicted by the buffer-derived line count — see F8. |
| Known-limitations 6 | This re-verification **overwrote the staged nine-line user-rules document** (a live save was part of staging F1); it now holds the two recoverable lines `! personal blocks` / `||tracker.example.com^`. The F3 probe policy was created and deleted; `kids`/`guest` are as staged. |

### Categories checked, no issue found

- **Plan compliance** — W1–W14 present; X1–X5 plus the four declared deviations
  verified against the artboards (labels, column order, precedence rows,
  MobileClients card structure grep-checked); `ownsHeader` seam as specified;
  Lists not retrofitted; `REFRESH_ENDPOINTS` unmoved; no new dependency, no new
  route, no `.md` touched.
- **Provenance** — every rendered figure traced to a field or a §8.5 row
  except F8's line count; no browser schedule arithmetic
  (`filtering-invariants.test.ts` + code read); `active_assignments` verbatim
  with the correct label; no per-assignment in-force claim (live DOM).
- **Recompile boundary** — §10.1 table verified against `routes.rs`
  (`Recompile::Yes/No` at each cited site); confirmations, blocking modal and
  no-abort on exactly the four recompiling operations; live mutations
  dialog-free; `PATCH` sends only changed fields, `lists: null` emitted as a
  literal null (double-option mirrored correctly).
- **Rule Tester** — D5's three branches, R3's block-with-zero-requests, the
  four verbatim fields, `user-rules` → "your custom rules", policy mode
  short-circuit — all consistent with `routes.rs:1250-1309`.
- **Bundle / scope** — 53.7 % of budget, four lazy chunks, no uPlot reference
  from this task's chunks, no Pi-hole strings, no phase-2.6 file in the staged
  set, no dev fixture files in the tree.
- **Memory / boundedness** — ring capped at 10 (pure, tested); no timer, no
  listener, no retained cross-page payload beyond component state.

### Fixes applied (2026-08-27) — F1–F9, owner-approved

F1 was fixed in Rust, per the owner's call: the root cause was server-side
ordering, and a frontend mapping would have been a second implementation of the
dedup that could drift.

| # | Fix | Where |
| - | --- | ----- |
| F1 | `put_user_rules` now validates the document **as sent**, then dedups on the success path only — 422 line numbers index the request. Frontend unchanged. | `routes.rs` `put_user_rules`; test `user_rules_422_line_numbers_index_the_document_as_sent` (`api.rs`) pins dup-above-invalid → `line 3`, not `line 2` |
| F2 | `onPopState` consults the block: while held, the browser entry is replaced with the path captured at `blockNavigation()` and nothing is announced — the page stays mounted, the modal stays up | `router.ts`; `router.test.ts` pins restore + no announce, and normal popstate after release |
| F3 | `lists: []` renders `no lists — blocks nothing` (`.lchip-warn`, warn token over words); the dialog refuses an empty subset (`pick at least one list`, submit disabled) | `policy-card.tsx`, `policy-dialog.tsx`, `components.css`; two tests in `policies.test.tsx` |
| F4 | `disposed` ref set in the effect cleanup; a mutation settling after unmount skips the re-read | `clients.tsx`; test: zero requests after unmount |
| F5 | `finally` clears `busyIp`/`renamingIp`/`assigningIp` per-ip via functional updates, never wholesale | `clients.tsx` |
| F6 | a failed re-read with data on screen renders `ErrorState` instead of silence, on Clients and Policies | `clients.tsx`, `policies.tsx`; test: stale rows + `did not reach the server` |
| F7 | the test POST carries an `AbortSignal` aborted at unmount; `AbortError` swallowed | `rule-tester.tsx` |
| F8 | an empty buffer is `0 lines`; T1 restated as the buffer line count | `rules.tsx`; test: empty document reads `0 lines` |
| F9 | both modals read "is being validated and, once accepted, … recompiled" — no acceptance claimed before the response | `rules.tsx`, `policies.tsx` |

**Verification.** `cargo fmt` / `clippy -D warnings` clean; `cargo test`
**1,196 passed, 0 failed** (+1: the F1 test); `npm run typecheck` clean;
`npm run test` **506 passed** (+6 over the review's 500); `npm run build`
**82,605 B gzip (53.8 %)**, brotli 73,529 — +198 B gzip for the three UI fixes.
The live `fah-p506` container still runs the pre-F1 binary; the F1 behaviour is
proven by the API test, not re-staged there.

Note: F1's fix means a duplicated **invalid** line is now reported once per
occurrence instead of once — accurate, and no existing test depended on the old
count.

### Fixes applied (2026-08-27, second round) — F10–F11, owner-approved

| # | Fix | Where |
| - | --- | ----- |
| F10 | `parseV6` takes the embedded v4 form only as the final 32 bits of the **whole** address — `1.2.3.4::` no longer parses, and `parseSelector` reads it as a `Name`, exactly as Rust does; the prefix-length check takes a leading `+` the way `u8::from_str` does (`/+24`) | `selectors.ts`; two pinning tests in `selectors.test.ts` (incl. `64:ff9b::192.0.2.1` and `::1.2.3.4:5` staying as they were) |
| F11 | `parseUserRulesError` rewritten as a **strict grammar parser**: each entry's quoted rule is walked as a Rust debug string (`\` escapes), so an anchor- or remainder-shaped span *inside* the quotes is content, never a new anchor; any message that does not parse end to end degrades to zero anchors + raw, as before | `validation.ts`; four new tests in `validation.test.ts` (embedded anchor, embedded remainder, escaped quotes, unterminated quote), all seven prior parser cases unchanged |

**Verification.** Frontend only — no Rust touched this round, the first round's
cargo run (1,196 passed, 0 failed) stands. `npm run typecheck` clean;
`npm run test` **512 passed** (+6); `npm run build` **82,701 B gzip (53.8 %)**,
brotli 73,616.

### Second-pass verification (2026-08-27) — full p5-06 → HEAD re-review after the fixes

Independent re-review of the complete range `03a81d8..1aba73d` (56 files), not
only the fix diffs. Ground truth re-read: `routes.rs` (`put_user_rules`,
`patch_policy`, `delete_policy`, `test_rule`, the `Recompile::` sites),
`wire.rs` (`PatchPolicyRequest` double options), `fah-rules/src/policy.rs`
`parse_selector`, `fah-model/src/policy.rs` `matches`/`specificity`/
`network_contains`, `fah-config/src/schema/policy.rs` `parse_days`/
`parse_time_of_day`. All gates re-run on this checkout, not read from the
sections above.

#### F1–F11 — independently confirmed fixed

| # | Re-verified by |
| - | -------------- |
| F1 | `put_user_rules` validates the joined document **as sent** before the dedup; dedup runs on the success path only; the stored text and the response are the deduped set, so T3's `sent.length − response.rules.length` still holds. `user_rules_422_line_numbers_index_the_document_as_sent` pins dup-above-invalid → `line 3`. The frontend sends `buffer.split('\n')` verbatim and anchors onto that same buffer — the two now index one document. |
| F2 | `onPopState` consults `navigationBlocks`; while held it `pushState`s the path captured at `blockNavigation()` and announces nothing, so the page stays mounted and `currentPath()` stays consistent for `navigate()`'s same-path check. Release → normal announce. `router.test.ts` pins restore, no-announce, and post-release popstate. History cannot grow unboundedly (each Back re-pushes the same held entry and clears forward history). |
| F3 | `ListChips` renders `[]` as `no lists — blocks nothing` (`.lchip-warn`, words not hue); the dialog's `subsetError` disables submit on an empty subset in **both** create and edit; `patch.lists` emits a literal `null` (pinned in `filtering.test.ts`), and `sameLists` distinguishes `null` / `[]` / order exactly as Rust's `target.lists != lists` does. |
| F4 | `disposed` ref set in the Clients effect cleanup; a mutation settling late skips `load()`; test `does not re-read after unmount when a mutation settles late` green. **Scope was Clients only — see F12.** |
| F5 | The `finally` clears `busyIp`/`renamingIp`/`assigningIp` per-ip via functional updates. Residual single-slot display noted as F14. |
| F6 | Both pages render `ErrorState` when `loadError !== null` with data on screen (`clients.tsx`, `policies.tsx`); test green. |
| F7 | The test POST carries `testController`'s signal, aborted in the effect cleanup; `AbortError` swallowed. |
| F8 | `buffer === '' ? 0 : buffer.split('\n').length`; empty-document test green. |
| F9 | Both modals read "being validated and, once accepted, … recompiled and swapped in" — no acceptance claimed before the response. |
| F10 | `parseV6` takes the embedded v4 form only as the final 32 bits of the whole address (`1.2.3.4::` → `Name`, `::1.2.3.4:5` still rejected, `64:ff9b::192.0.2.1` still an address); the prefix suffix takes `/^\+?[0-9]+$/` with the >255 u8 refusal — both line-checked against `parse_selector` and `Ipv4Addr`/`Ipv6Addr::from_str` semantics, tests pinned. |
| F11 | `parseStrict` walks each quoted rule as a Rust debug string (`\` escapes), `TAIL` accepted only as the final segment, any grammar violation → zero anchors + raw. Embedded-anchor, embedded-remainder, escaped-quote and unterminated-quote tests green. |

#### Gates, re-run on this checkout

`cargo fmt --all -- --check` clean · `cargo clippy --workspace --all-targets
-- -D warnings` clean · `cargo test --all-features --workspace` **1,196
passed, 0 failed, 8 ignored** · `npm run typecheck` clean · `npm run test`
**512 passed in 40 files** · `npm run build` **82,701 B gzip (53.8 %), brotli
73,616 B** — every figure byte-identical to the second-round claims above.
`dist/` grep: no `pi-hole`/`pihole` anywhere; `uplot` only in
`uPlot.esm-*.js` and the p5-06 CSS scope — no chunk from this task references
it.

#### Whole-plan regression pass — categories checked, no issue found

- **Recompile boundary vs Rust** — §10.1 re-verified against the live
  `Recompile::` sites (`routes.rs:907/933-938/969/1012/1036`); the frontend's
  `sameLists` and the handler's `target.lists != lists` agree on `null` vs
  `[]` vs order, so confirm/block and the actual rebuild cannot disagree on
  one snapshot. A stale page copy can only over-confirm, never under-confirm.
- **Clients request discipline** — one `GET /clients` + one `GET /policies`
  on mount, zero `GET …/policy` (test-pinned); no `getClientPolicy` accessor
  (invariants test).
- **Assignment classification** — `selectors.ts` and `assignment.ts`
  re-checked line-for-line against `parse_selector`, `matches`,
  `network_contains` and §7.3 (branch order R1, 2a′ refusal, both branch-0
  shapes, string-equality direct lookup, first-match order); `viaText`'s two
  spellings and 2b's sole-candidate append match §8.4. `parseDays`/
  `parseTimeOfDay` mirror `schema/policy.rs` including the first-dash split
  and the `u8`/`u16` `+`-sign quirks.
- **Rule Tester** — D5's three branches, R3's zero-request block, policy-mode
  short-circuit, `user-rules` naming, all consistent with
  `routes.rs:1258-1298` (name → bare `ClientContext`, explicit `policy` wins).
  The empty-client-field case is F13.
- **Patch shapes** — `PatchPolicyRequest.name` is a plain option, so a name
  cannot be cleared via the API; the form's inability to clear one mirrors the
  API rather than hiding a capability. Double options emitted as literal
  `null`s, pinned.
- **events/endpoints/timers** — all four routes declare `[]`/`[]`;
  `filtering-invariants.test.ts` + `routes.test.ts` pin the absences and the
  `REFRESH_ENDPOINTS` five. The V2a server-side connection count and the
  V14–V17 browser measurements were **not re-staged in this pass** (no live
  container run); they stand on the second-round re-verification recorded
  above.
- **Scope hygiene** — staged range holds p5-07 paths, the plan file, the
  phase-table status and this review only; the modified `phase2.6` review file
  remains uncommitted per plan §2; no dev fixture in the tree.

#### New findings

##### F12 — Should-fix — FIXED (2026-08-27) — Policies re-reads from a dead page; F4's fix was applied to Clients only

`policies.tsx` `run()` chains `.then(() => reloadPolicies())` with no
`disposed` guard — the exact shape F4 fixed in `clients.tsx:120-122`. A
non-recompiling `PATCH` (rename, blocking-mode, assignments — nothing blocks
navigation for these) settling after unmount calls `reloadPolicies()`, which
creates a **fresh** `AbortController` and issues a `GET /policies`
attributable to the unmounted route, with a controller nothing will abort (the
effect cleanup aborted the previous one before `reloadPolicies` replaced it).
Violates the phase invariant "leaving a page stops its work" in the same
mutate-then-navigate window F4 named. Recompiling mutations are unaffected —
the navigation block keeps the page mounted. Inferred from code, not staged.
**Smallest fix:** the same `disposed` ref pattern `clients.tsx` uses, checked
before `reloadPolicies()`.

##### F13 — Minor — FIXED (2026-08-27) — client mode with an empty client field renders the policy-mode "why"

`QueryForm`'s submit is enabled on a non-empty domain alone, and
`rule-tester.tsx:160-172` sends `{domain, qtype}` when the client field is
blank. The record then has `sentPolicy: null`, `partial: false`,
`sentClient: null`, so `send()` falls through to `explain(null, …)`, which
returns `CHOSEN_POLICY_REASON` — the result card prints **"you chose this
policy — assignments are ignored"** for a test where no policy was chosen and
client mode was selected. §8.4's table has no row for this shape and the
sentence it borrows belongs to policy mode; the session ring also renders
`as` followed by nothing. The verdict itself is honest (default context is
what the engine uses). **Smallest fix:** its own sentence — e.g.
`no client given — the default policy decides` — or require the field before
submitting in client mode.

##### F14 — Nitpick — FIXED (2026-08-27) — `busyIp` is a single slot, so a second row's mutation blanks the first row's busy state

F5's fix stopped the *clearing* from being wholesale, but `busyIp` still holds
one ip: starting a mutation on row B while row A's is in flight repoints it to
B, so row A renders idle mid-flight and its buttons re-enable — a second,
overlapping mutation on A becomes startable, and the first A-run's per-ip
`finally` then clears the second A-run's state. Display-only: the mutations
are idempotent PUTs/DELETE, the re-reads settle every row, and no data is
wrong. **Smallest fix:** a `Set` of busy ips (add on start, delete own entry
on settle).

### Fixes applied (2026-08-27, third round) — F12–F14, owner-approved

| # | Fix | Where |
| - | --- | ----- |
| F12 | `disposed` ref set in the effect cleanup; a mutation settling after unmount skips `reloadPolicies()` — the same pattern F4 put on Clients | `policies.tsx`; test `does not re-read after unmount when a live mutation settles late` (`policies.test.tsx`) |
| F13 | a blank client field gets its own sentence — `no client given — the default policy decides` — never policy mode's; `explain()` tightened to a non-null address; the session ring prints `as the default policy` instead of `as ` | `rule-tester.tsx`, `session-ring.tsx`; test pins the request body (`{domain, qtype}`, no `client`), the "why" row and the ring row |
| F14 | `busyIp` replaced with a `ReadonlySet<string>`: added on start, own entry deleted on settle, so two rows in flight both read busy and a same-ip re-entry cannot start while its first run is pending | `clients.tsx`; test `keeps both rows busy while two mutations are in flight` settles the two in order and asserts each glyph re-enables on its own settle |

**Verification.** Frontend only — no Rust touched, the first round's cargo run
(1,196 passed, 0 failed) stands. `npm run typecheck` clean; `npm run test`
**515 passed in 40 files** (+3); `npm run build` **82,754 B gzip (53.9 %)**,
brotli 73,675 — +53 B gzip for the three fixes.

### Status

**PASS** — F1–F14 all fixed and verified. The V2a/V14–V17 live rows and V19's
heap half stand on the earlier runs as recorded in Known limitations.

---

## Live UI test pass (2026-08-27) — Playwright, no code read for verdicts

Driven against the running app: Vite dev server on `:5173` proxying to
`fah-p506` (`fastadhunter:p5-05`, published on 18443). Every row below is read
off the live DOM, a live request log, or the API container's own network
namespace. **No code was reviewed in this pass** — source was opened only to
build a faithful stub (the 422 envelope shape) and to confirm the image
predates the F1 fix.

**Two method notes.**

1. **Viewport.** `browser_resize` is a no-op on this browser (window follows
   viewport), so 390 / 600 / 767 / 768 / 900 / 1024 px were measured in a
   same-origin iframe of the app sized in CSS pixels — real media queries, real
   layout. 1400+ was measured in the top-level page (1555 CSS px).
2. **Mutations.** Live writes to the container were refused by the harness, so
   every write in this pass was intercepted in the browser: the request is
   built and sent by the page, then held pending or answered from a stub. What
   this proves is the **UI contract** — what is sent, what is shown, what is
   blocked; it does not re-prove server behaviour. The three checks that need a
   real write (F1 end to end, V8a's wall-clock timings, the real `409`) are
   listed as not reproduced.

### Re-verified live — claims that held

| Claim | Measured |
| ----- | -------- |
| V2 / V2b | all four routes: `fahUnion()` `[]`, `fahSocketState()` `closed`, `fahTimers()` `0` |
| V2a | parked on `/clients`: **0** ESTABLISHED to the API inside its own netns (`docker run --network container:fah-p506 busybox netstat -tn`) |
| V3 | Dashboard, each of the four, back to Dashboard: union `['stats']`, socket `open`, timers `5` restored |
| V4 / V5 | fresh `/clients`: exactly `GET /api/v1/clients` + `GET /api/v1/policies`; zero `GET .../policy` |
| V6 / V6a | five branches live: `.pchip` **solid** for direct (`kids` on 172.17.0.5), `.pchip.inh` **dashed** for inherited; notes `daily · 20:00 -> 22:00 · in force`, `via name printer`, `via 172.17.0.8/32`, bare `inherited` (two selectors match .10), `inherited · no assignment` |
| V7a (R1 order) | 172.17.0.11: direct `kids` window **shut** *and* a schedule-less name assignment — row reads `not in force — the name assignment on "console" decides (guest)`, with **no** `window shut` claim |
| X2 | `ACTIVE NOW` absent from the live Policies DOM |
| V8 | `Change which lists this policy holds?` naming `seconds of CPU on the router`; **0** requests before the confirm; after it `PATCH /api/v1/policies/kids :: {"lists":["oisd-basic"]}` — changed field only; busy modal up; `navigate('/clients')` refused. Delete confirm: `Delete kids?` naming its **2** assignments and the rebuild; 0 requests before the confirm, 0 on Cancel |
| F2 | with the PATCH pending: `history.back()` twice and `history.forward()` all left the path on `/policies`, modal up, `history.length` steady at **13**, no second request |
| F9 | modal reads `The policy set is being validated and, once accepted, the whole ruleset is recompiled and swapped in...` — no acceptance claimed |
| F3 | dialog: `only these` with nothing ticked gives `pick at least one list — an empty subset gives this policy no list at all`, submit disabled. Card: a policy with `lists: []` renders `no lists — blocks nothing` in `.lchip-warn` |
| F4 / F12 | mutation held pending, navigate away, then settle: **zero** requests, on Clients *and* Policies |
| F5 / F14 | two rows in flight at once both read busy; settling A re-enabled **only** A and fired its own re-read pair; settling B re-enabled B |
| F6 | re-read failed after a successful mutation: `The API did not answer / GET /api/v1/clients did not reach the server` above **26** stale rows still on screen |
| F13 | blank client field: body `{"domain":...,"qtype":"A"}` with no `client`; why-row `no client given — the default policy decides`; ring row `as the default policy` |
| F8 | empty buffer reads `0 lines` |
| F11 | `line 6: invalid rule syntax: "line 5: invalid rule syntax: \"x\""` yields exactly **one** anchor (line 6); no line-5 anchor |
| V1 / V1b | 422 on line 7 of a 14-line document: band exactly over line 7 (band top 322.06 against a computed 322.07, height 21.99 against a 22 px line), buffer byte-identical, error entry a **765 x 44** button that focuses the textarea and selects exactly `@@||^`. Unparseable envelope: **0** anchors, **0** callouts, raw text in the banner, `2 lines` with no invalid count |
| V9 / V10 | `default` gives `"default" names the implicit policy...`, submit disabled; `Kids Zone!` gives the API's own alphabet message; with 15 policies staged: `16 / 16`, `0 policy slots left` plus the 16-bit reason, **both** `New policy` buttons disabled |
| V12 / V12a / V12b | client mode by address: `via name printer`, policy `guest`; policy mode: body carries `policy` and no `client`, why-row `you chose this policy — assignments are ignored`; ambiguous `TV`: `Which one?` listing `172.17.0.5 tv` / `172.17.0.6 TV` with **zero** `POST /rules/test`, and choosing one sent that address |
| Ring bound | 12 tests leave **10** rows, newest first |
| V14 | `scrollWidth - clientWidth` = **0** on `documentElement` and `.main`, all four routes, at 1555 / 900 / 390, dark **and** light. `.clients-scroll` is the only internal scroller: **104 px** at 900, **0** at 390 and at desktop |
| V15 (390 only) | zero undersized controls on all four routes; drawer `Theme` **34.5 x 44**, `Sign out` **42.5 x 44** — the two pre-existing p5-05 exceptions, unchanged |
| V16 | glyph **44 x 44**; tap expands in place, `Rename` / `Change policy` **153.7 x 44** on one row, `location.pathname` unchanged, desktop header `display: none`; rename editor input 181 x 44, `Save` 53.6 x 44, `Cancel` 64.7 x 44 |
| V17 | 390 px: gutter **34 px**; a 60-line document scrolls **inside** the editor (`.editor-area` 1320 / 572) with the page still at 0 horizontal overflow; summary two-up; result `kv` single-column |
| Clients grid | header against row: identical `left` and `width` on **all 8** columns (0.0 px difference), 10 px gutters, last track exactly **44 px** |
| Search | name match (`printer`, 1 row), address prefix (`172.17.0.1`, 10 rows), `No client matches that search`, clears back to 25 |
| Artboard fidelity | `Clients.dc.html` column labels and order match one for one; `RuleTester.dc.html` field labels, result rows and ring-row shape (`as` / `under policy`) match; `CustomRules.dc.html` banner, `rules.txt` card, `N lines · N invalid` and the Precedence trio match |

### New findings

#### F15 — Should-fix — a 422's anchors survive editing and then point at the wrong line

The anchors, the band, the banner and the `· N invalid` count are cleared only
by the next save, never by an edit. Staged live on `/rules` (422 stubbed on
line 3 of a 3-line document, nothing written):

| Buffer after the 422 | Band | Callout | Counter | Banner |
| -------------------- | ---- | ------- | ------- | ------ |
| line 3 fixed to a valid rule | still line 3 | `line 3 — invalid rule syntax: "@@||^"` | `3 lines · 1 invalid` | `fix line 3 and save again` |
| shrunk to 1 line | still at y 322.1 — **below the last line** | unchanged | `1 line · 1 invalid` | unchanged |
| emptied | unchanged | unchanged | **`0 lines · 1 invalid`** | unchanged |

Clicking the stale entry on the 1-line buffer focused the textarea and selected
**line 1** — a valid line, silently. That is the same failure F1 was raised for
(an anchor over content that is not the reported one), reached by editing
instead of by the dedup. `0 lines · 1 invalid` is also a self-contradicting
figure.

**Smallest fix:** clear the errors — or at least drop the band and callout — on
the first `input` after a failed save.

#### F16 — Should-fix — the floating callout hides the whole content of the line beneath it

Measured with a 14-line document and an invalid line 7, at both widths:

| Width | Band (line 7) | Callout | Overlap |
| ----- | ------------- | ------- | ------- |
| 1555 px | y 322.06, h 21.99 | y 344.06, h 19.99, w 203.3 | covers 20 of line 8's 22 px, from its left edge |
| 390 px | y 557.3, h 22 | y 579.3, h 20, w 203.8 of a 302.7 px editor | same, 67 % of the line's width |

The callout is opaque: at both widths line 8's text `@@||goodsite.example.com^`
is **not visible anywhere** — the gutter still numbers 7, 8, 9 but row 8 shows
only the callout. `CustomRules.dc.html` renders the same callout as its own row
between line 7 and line 8, so no document content is hidden there. Section 7.1
declares "overlaying the line beneath it" as the shipped treatment, but neither
it nor V1 says the covered line's text becomes unreadable, and the artboard
does not do that.

**Smallest fix:** insert the callout as a row and push the following lines down,
or offset it so the covered line stays readable.

#### F17 — Should-fix — the 44 px touch rule is width-gated at 767 px, so tablets get 25.6 px controls

Same page, four widths, `/policies`:

| Width | `Edit` | `Delete` | `New policy` | Clients search box |
| ----- | ------ | -------- | ------------ | ------------------ |
| 1024 | 41.3 x **25.6** | 56.1 x **25.6** | 90.3 x **33.6** | 220 x **34** |
| 768 | 41.3 x **25.6** | 56.1 x **25.6** | 90.3 x **33.6** | 220 x **34** |
| 767 | 47.2 x 44 | 62.1 x 44 | 725.6 x 44 | at least 44 |
| 600 | 47.2 x 44 | 62.1 x 44 | 559 x 44 | at least 44 |

768 and 1024 are iPad portrait and landscape. V15's PASS was produced at 390 px
only and does not cover the range where a touch device is most likely to meet
the desktop layout. Nothing keys off `pointer: coarse`; the sizing flips purely
on width, between 767 and 768.

**Smallest fix:** add `@media (pointer: coarse)` to the rule the 767 px
breakpoint already carries.

#### F18 — Minor — the busy modal never takes focus

On mount `document.activeElement` is **BODY**. The modal itself is
`role="dialog" aria-modal="true" aria-busy="true" aria-label="Rebuilding the
ruleset" tabindex="0"` and the trap does work — Tab lands on the modal, a
second Tab returns to BODY, the sidebar behind it is never reached, and Escape
does not dismiss — but nothing places focus inside it when it appears. Both
dialogs that precede it do it correctly (edit dialog to `#policy-name`, confirm
to `Cancel`). A screen-reader user gets no announcement that a blocking
operation started; a keyboard user has to press Tab to find out where they are.
The review's "one focusable element so the trap has somewhere to put focus" is
half true: the element exists, nothing focuses it.

#### F19 — Minor — the four new routes show no health signal at all

| Route | Pill | Class | Dot |
| ----- | ---- | ----- | --- |
| `/rules`, `/policies`, `/clients`, `/rule-tester` | `not needed here` | `conn not-needed-here` | grey `rgb(138,149,163)` |
| `/lists`, `/` | `live` | `conn live` | green `rgb(61,154,99)` |

The pill is the only always-visible statement that the resolver is up. On the
four routes this task ships it answers a different question — "this page needs
no socket" — in the place where the operator reads health, so a resolver that
has died is indistinguishable from one that is fine for as long as they stay on
these pages. Raised by the repo owner from the running UI.

**Options:** keep reporting the last known health (the shell already holds it),
or state reachability from the page's own reads rather than from a socket it
deliberately does not open.

#### F20 — Nitpick — the Rule Tester's `tested as` prints the typed name, not the resolved client's

Staged with the two observed clients `tv` (172.17.0.5) and `TV` (172.17.0.6).
Typing `TV` and then choosing `172.17.0.5` from the prompt produced
`tested as 172.17.0.5 (TV)` — but 172.17.0.5 is named **`tv`**, and `TV` is the
name of the *other* client. The parenthetical reads as the resolved client's
identity and is not.

Two smaller things in the same flow: the client field still holds `TV` after
the address is chosen (the address appears only on the result card), and the
**previous** result card stays on screen underneath the `Which one?` prompt,
still answering the earlier query.

#### F21 — Nitpick — two guards the pages could apply locally and do not

- `Validate and save` is **enabled on an unchanged buffer** (`Discard` is
  correctly disabled), so a recompiling `PUT /rules/user` can be fired for a
  no-op.
- The create-policy form mirrors three server validators locally (`default`,
  the id alphabet, the empty subset) but not **duplicate id**: typing `kids`,
  which is in the list the page already holds, leaves `Create policy` enabled
  and shows nothing; the `409` is only discovered after the round trip.

#### F22 — Nitpick — blocked-share bars use the accent hue where both artboards use the bad hue

| Source | Fill |
| ------ | ---- |
| live `policy-traffic-fill` and `c-share-fill` | `rgb(31,157,187)` (accent) |
| `Policies.dc.html` lines 91/117/138, `Clients.dc.html` lines 80/89/98 | `#d1504b` (bad) |

Proportions are right (`0.108 x 267.4 = 28.8 px` for a `10.8 %` row, so T9
holds) and the two pages agree with each other; they disagree with both
artboards, and the Deviations section does not list it.

### Corrections to earlier sections

| Section | Correction |
| ------- | ---------- |
| V15 | Its PASS covers 390 px only. At 768 to 1024 px this task's own controls measure 25.6 to 34 px — see F17. |
| V10 | Only the **header** `New policy` carries the ceiling in its `title`; the second one has no `title` (the reason is printed above it). Both are disabled, as claimed. |
| V17 | The result card was measured **30.7 px** below the form at 390 px (form bottom to card top), not 16 px. |
| Section 7.1 / V1 | The shipped callout does not merely overlay the next line, it makes that line's text unreadable — see F16. |
| Fixes applied, F1 | The running `fah-p506` image was built **2026-08-26T20:46Z**, before the fix commits, so F1 remains proven by the API test only. The frontend half was re-proven here against a stubbed `line 3` (dup, dup, invalid anchors line 3, buffer byte-identical). |

### Not reproduced in this pass

| What | Why |
| ---- | --- |
| F1 end to end | the live container predates the fix and rebuilding it was out of scope |
| V8a wall-clock timings (33 / 26 / 13 / 12 / 11 ms) | live writes were refused by the harness; every mutation here was held or stubbed |
| the real `409` on a duplicate policy id | same |
| V19's heap half | unchanged from the original run, still needs a forced GC |

### Status after this pass

**PASS WITH DEFERRED FINDINGS** — F1 to F14 all hold up under live re-test.
Three new should-fix items (F15 stale anchors, F16 hidden line, F17 tablet
touch targets) and five smaller ones (F18 to F22) are open.

### Fixes applied (2026-08-27, fourth round) — F15–F22, owner-approved

F19 was fixed the way the owner chose: the pill reports the API on routes that
open no socket, rather than reporting the absence of a subscription.

| # | Fix | Where |
| - | --- | ----- |
| F15 | editing the buffer drops the whole rejection — anchors, band, banner and the `· N invalid` count — so a line number can never address text it was not measured against | `rules.tsx` (`edit`); test `drops the whole rejection as soon as the text changes` |
| F16 | the callout moved onto the offending line's **own** row, starting one column after that line's text (`min(calc(4px + N ch), 60%)` on a row that carries the editor's monospace face, so `ch` is exact). No document line is covered any more; a long bad line clamps the message over its own tail, never over another | `line-editor.tsx`, `components.css` (`.editor-callout-row`); test pins row top = band top and the padding |
| F17 | `@media (pointer: coarse)` raises the controls the ≤ 767 px blocks raise, so the rule follows the pointer instead of the width | `components.css`; test `styles/cascade-invariants.test.ts` |
| F18 | `focusableWithin` counts the container when the container is itself focusable — `querySelectorAll` never returns the node it is called on, which is why the busy modal (whose only focusable element is its root) got no focus | `focus-trap.ts`; test `moves focus into the busy modal` |
| F19 | `request()` publishes whether the API answered (`apiReach` / `subscribeApiReach`); the shell shows `live` for `not-needed-here` when it has, and a new `api-unreachable` state — `API not answering`, problem-coloured — when a read did not land. Event-driven, **no timer**: `activeTimers()` stays 0 on all four routes | `api/core.ts`, `api/index.ts`, `events/types.ts`, `shell/shell.tsx`, `connection-indicator.tsx`, `layout.css`; tests in `api/core.test.ts` |
| F20 | `tested as` names the client that was picked, not the string that was typed; the field takes the chosen address; the previous result card comes down while a choice is pending | `rule-tester.tsx`; two tests in `rule-tester.test.tsx` |
| F21 | `Validate and save` is disabled on an unchanged buffer, like `Discard`; the create form refuses an id the page already lists, in the handler's own wording, with no request sent | `rules.tsx`, `policy-dialog.tsx`, `policies.tsx`; three tests |
| F22 | the two blocked-share fills are qualified by their track (`.bar > span.c-share-fill`), so they stop losing the cascade to `.bar > span`'s accent — the bars were painted `--series-permitted` while labelled "blocked share" | `components.css`; test in `styles/cascade-invariants.test.ts` |

**Verification.** Frontend only — no Rust touched, `git status -- crates/`
empty, so the first round's cargo run (1,196 passed) stands. `npm run
typecheck` clean; `npm run test` **525 passed in 41 files** (+10); `npm run
build` **83,236 B gzip (54.2 %)**, brotli 74,085 — +535 B gzip.

The `409` test on Policies was rewritten rather than deleted: an id this page
does not list is still refused by the server (another session can create one
between the read and the write), and that path keeps its test.

#### Re-measured live after the fixes

Same harness as the pass above: Vite dev server against `fah-p506`, writes
held or stubbed in the browser, container state verified unchanged afterwards.

| # | Live result |
| - | ----------- |
| F15 | 422 on line 7 of a 14-line document, then edits: fixing line 7 → **0** callouts, 0 bands, 0 error entries, no banner, counter back to `14 lines`; shrinking to 1 line → `1 line`; emptying → `0 lines`. No `· 1 invalid` survives an edit |
| F16 | callout row top **557.3** = band top 557.3 — the same row. Callout spans x 105.9–357.8 inside an editor starting at 60.7, i.e. it begins 45 px in (4 px padding + five columns of `@@||^`). Line 8 (`@@||goodsite.example.com^`) renders in full — screenshot in `.playwright-mcp/fix-f16-callout.png` |
| F17 | 1024 × 768 with `(pointer: coarse)` true: **zero** controls under 44 px on Policies, Clients and Custom Rules — including the two top-bar controls, whose bar stays 58 px tall. With the emulation cleared the desktop sizes return (`Discard` 33.5, `theme` 18.1) |
| F18 | focus lands on the modal itself (`DIV.dialog`, `aria-label="Rebuilding the ruleset"`) the moment it mounts; `Escape` still does not dismiss |
| F19 | `/rules`, `/policies`, `/clients`, `/rule-tester`: `conn live`, green `rgb(61,154,99)`. With reads failing: `conn api-unreachable`, `API not answering`, problem colour. Reads recover → `live` again. `fahTimers()` **0** at every step |
| F20 | typing `TV`, choosing `172.17.0.5`: `tested as 172.17.0.5 (tv)` — the resolved client's own name — deciding policy `kids`, field now `172.17.0.5`. While the prompt was up the previous result card was **absent** |
| F21 | untouched document: `Validate and save` **disabled** alongside `Discard`; after one edit, enabled. `kids` and `guest` in the create form: `policy kids already exists` under the field, submit disabled, **zero** requests sent |
| F22 | `c-share-fill` and `policy-traffic-fill` both computed `rgb(209,80,75)` = `#d1504b`, the artboards' hue; widths unchanged (28.8 of 267.4 px for a 10.8 % row) |

### Status after the fixes

**PASS** — F1–F22 fixed and verified. The V2a/V14–V17 rows and V19's heap half
stand on the runs recorded above; F1 end to end still needs a container built
from a commit that carries its fix.
