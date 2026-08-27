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

### Status

**PASS** — F1–F11 all fixed and verified; nothing open.
