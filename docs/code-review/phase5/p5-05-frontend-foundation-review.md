# P5-05 — Frontend Foundation · Review

**Task:** [p5-05-frontend-foundation.md](../../../plan/wip/phase5/p5-05-frontend-foundation.md) ·
**Plan:** [p5-05-frontend-foundation-plan.md](../../../plan/wip/phase5/p5-05-frontend-foundation-plan.md) ·
**Branch:** `phase5-05` · **Status:** implementation complete, awaiting `start code review`

---

## Implementation Summary

### What was implemented

The application shell, end to end. `dashboard/frontend/` is a Vite + TypeScript +
Preact project that builds a static bundle, is baked into the image by the
Dockerfile's `frontend` stage, signs in against `p5-04`'s session routes, holds a
route-scoped `WS /api/v1/events` connection, and fails its own build over the
150 KB gzip budget. Thirteen sidebar entries resolve; none of the product screens
is built, and by construction none of them declares an event type or a polled
endpoint until its own task lands.

Implementation followed §21 unit by unit — U1 through U15 — each ending with
`npm run typecheck`, `npm run test` and `npm run build` green before the next
began. U16 needed no work: §23's documentation edits were already applied and
committed.

### Files and modules

| Area | Files |
| ---- | ----- |
| Project | `package.json`, `package-lock.json`, `tsconfig.json`, `vite.config.ts`, `index.html`, `public/favicon.svg` |
| Post-build | `scripts/postbuild.mjs` (+ `postbuild.test.mjs`) |
| API client | `src/api/{core,types,auth,health,telemetry,cache,index}.ts` (+ `core.test.ts`) |
| Events | `src/events/{types,subscriptions,backoff,probe,socket}.ts` (+ four test files) |
| Refresh | `src/refresh/{registry,preferences,use-refresh}.ts` (+ two test files) |
| Lifecycle | `src/lifecycle/{timers,visibility,route-lifecycle}.ts` (+ three test files) |
| Router | `src/router/{router,routes,link}.ts(x)` (+ two test files) |
| Session | `src/session/{session,guard}.ts(x)` (+ `session.test.ts`) |
| Shell | `src/shell/{shell,sidebar,topbar,content-header,connection-indicator,icon}.tsx` (+ `sidebar.test.tsx`) |
| Components | `src/components/{tile,card,table,verdict-pill,status-pill,stage-bar,empty-state,error-state,confirm-dialog,chart,refresh-cluster,data-age}.tsx` (+ `refresh-cluster.test.tsx`) |
| Pages | `src/pages/{login,not-yet-built,system,dev-gallery}.tsx` (+ two test files) |
| Styles / assets | `src/styles/{tokens,base,layout,components}.css`, `src/assets/sprite.svg` |
| Wiring | `src/{main.tsx,app.tsx,constants.ts,services.ts}` |
| Repository | `Dockerfile` (frontend stage rewritten), `.gitignore` (two entries), `dashboard/.gitkeep` deleted |

No Rust source changed. No new API route, no new config key.

### Design decisions

**Three modules were added beyond §1's file list**, each because the plan's own
requirements needed a seam that list did not name:

- `lifecycle/route-lifecycle.ts` — §10's transition. The plan places it "in the
  shell"; extracting it is what makes §19's *Connection ownership* and
  *Timer module* cases testable without a browser.
- `services.ts` — the four long-lived objects (subscription registry, refresh
  registry, socket manager, route lifecycle), constructed once. Building them
  inside a component would let a re-render duplicate the socket.
- `shell/icon.tsx` — the sprite `<use>` wrapper, so exactly one module knows the
  hashed asset URL.

**Chunking.** The production build emits `index`, `login` and `system` — three
meaningful chunks — plus one stylesheet and the hashed sprite. **No `charts`
chunk exists in a production build of this task**, and that is deliberate rather
than a miss: no shipped page renders a chart, so emitting a chunk nothing fetches
would be weight for nothing. The split mechanism is real and proven — `chart.tsx`
is the only module that touches uPlot and it does so through `await import()`,
the dev gallery renders it, and the postbuild verifies no production asset
mentions uPlot. p5-06 lands the first real chart and with it the fourth chunk.

**`built` on the route table.** Each route carries its target `events` and
`endpoints` columns *and* a `built` flag; `effectiveEvents`/`effectiveEndpoints`
return the declaration only once the screen exists. This keeps §9's table
complete and legible while satisfying D7 — an unbuilt screen acquires nothing —
without a second, driftable copy of the mapping. A vitest case pins `query` to
exactly one route for the life of the phase.

**The dev gallery is excluded by a guard, not by tree-shaking.**
`GALLERY_ROUTE.load` is `import.meta.env.DEV ? () => import(…) : null`. An
unguarded `import()` would still emit the chunk, because reachability is decided
statically. The postbuild grep asserts the marker string is absent from every
emitted file.

**`vite.config.ts` reads `FAH_API_TARGET`** (defaulting to
`https://localhost:8443`) so development can point at an API on another port.
The explicit `Origin` header §6 requires is unchanged and is what makes the dev
WebSocket work at all.

**Dark palette.** The artboards draw light only. visual-system.md decides where
they are silent, so the dark set is derived from the sidebar surfaces the
artboards *do* draw dark, defined once in `tokens.css` and again under
`prefers-color-scheme` for the case where the pre-paint script's `try`/`catch`
swallowed a blocked-storage read.

### Tests

`npm run test` — **200 vitest cases in 19 files**, covering every area §19 lists:
subscription refcounts and the empty-union close, backoff schedule and jitter
bounds, the `OPEN_STABLE_MS` reset, immediate-failure detection, probe
classification and its one-shot/cooldown behaviour, the upgrade-refused
diagnostic at two cycles and not one, refresh sharing and last-unsubscribe timer
clearing, retained-value-is-not-a-subscription, interval-preference validation
and cross-tab propagation, manual-refresh coalescing, the age ticker's refcount,
the timer-module grep, envelope validation, the route table's mapping, API-core
envelope and `Retry-After` handling, the `401` guard, the login failure set, and
the size-gate comparison.

Workspace: `cargo fmt --check` clean, `cargo clippy --workspace --all-targets -D
warnings` clean, `cargo test --workspace` **1,195 passed, 0 failed** across 44
suites.

### Measurements

Taken against `fastadhunter:p5-05` (amd64, this commit's tree), API version
0.2.20, Chromium 1228 headless, Docker Desktop 29.7.2 on the dev box. These are
dev-box figures for a browser and a bundle; nothing here is an RB5009
measurement.

**Bundle — final, gzip gates, brotli is what travels:**

| file | raw | gzip | brotli |
| ---- | ---: | ---: | ---: |
| `assets/index-*.js` | 36,654 | 12,916 | 11,728 |
| `assets/style-*.css` | 14,240 | 3,705 | 3,229 |
| `assets/sprite-*.svg` | 3,733 | 858 | 750 |
| `assets/login-*.js` | 1,852 | 938 | 801 |
| `index.html` | 1,130 | 613 | 432 |
| `favicon.svg` | 258 | 185 | 163 |
| `assets/system-*.js` | 135 | 132 | 92 |
| **TOTAL** | **58,002** | **19,347** | **17,195** |

**19,347 B gzip against the 153,600 B budget — 12.6 %.**

Dependency weight, measured by gzipping the shipped module files:
`preact` 4,834 B gzip (+ `preact/hooks` 1,545 B), `uplot` 41,845 B gzip which
**this build does not ship at all**. Pinned exactly, as instructed: preact
10.29.8, uplot 1.6.32, vite 8.2.2, typescript 7.0.2, @preact/preset-vite 2.10.6,
vitest 4.1.11, jsdom 30.0.1, @types/node 26.3.0. TypeScript 7 (the native port)
built and typechecked the whole project without incident.

**Image:** the `/web` layer is **164 kB**. The runtime filesystem holds 23
entries under `/web` and **zero paths matching `node`**. Note the total reported
by `docker images` for this build is 25.8 MB uncompressed-on-disk, which is not
the same measure as p5-01's 13.0 MiB figure; the honest comparable is the
delta — the bundle adds 164 kB.

### Verification — V1–V16 plus V9a, V9b, V9c, V11a, V11b

Read off a request log, a WebSocket frame log and the server's own connection
table against a **running** container. Production-origin checks ran against
`https://localhost:18443`; checks that need a route which actually renders data
ran against the dev server and the dev gallery, because no shipped p5-05 route
declares an event type or a polled endpoint.

| # | Result |
| - | ------ |
| **V1** | Shell served `200 text/html`, `cache-control: no-cache`, `vary: accept-encoding`. `/settings` deep link resolves through the SPA fallback. `content-encoding: br` when offered (11,669 B), `gzip` when only gzip is offered (12,838 B). |
| **V2** | One request per emitted extension: `.js` `text/javascript`, `.css` `text/css`, `.svg` `image/svg+xml`, `.html` `text/html`. `/assets/*` `immutable`, everything else `no-cache`. **No `.json` asset is emitted**, so that extension is untested — there is nothing to test. |
| **V3** | `POST /auth/login` → `204` + `Set-Cookie: __Host-fah_session=…; Secure; HttpOnly; SameSite=Strict; Max-Age=604800`. Browser sign-in lands on `/`. Wrong password renders "That password is not right." from the envelope. |
| **V4** | Gallery open, API restarted underneath it: 1 → 2 sockets, frames sent `["{"subscribe":["stats"]}","{"subscribe":["stats"]}"]`, indicator observed `reconnecting` then `live`, no navigation to login. |
| **V5** | 20 s subscribed to `stats` with the API running: received frame types `["stats"]` only, 10 frames. Zero `query` frames. |
| **V6** | (a) cookie deleted → next authenticated call → `/login`. (b) server unreachable 12 s → **stays** on the route, no navigation to login. |
| **V7** | **Server-side**, counted inside the container's own network namespace: event-free route **0**, gallery **1**, back on the event-free route **0**. The indicator reads `not needed here` on every shipped route. |
| **V8** | Entering the gallery opens one socket and sends `{"subscribe":["stats"]}`; leaving closes it (1/1) and opens nothing new. |
| **V9** | Two cards on `/cache` and two on `/telemetry` → **1 request each**. 15 s on an unbuilt route → **0** requests. 20 s after leaving the gallery → **0** requests. |
| **V9a** | Changing `/telemetry` from `5 m` to `1 m` issued **0** requests and stored `60`. Cluster placement matches the artboards (see below). |
| **V9b** | Three rapid Refresh clicks → **1** `/cache` request. |
| **V9c** | Both polled endpoints on the page show an age; it advances without a reload (unit-tested against the shared ticker). |
| **V10** | Hidden → polling stops at once, socket closes after 30 s, **0** requests while hidden. Hide/show inside the grace → **0** new sockets, socket never closed. Becoming visible re-sent `{"subscribe":["stats"]}`. |
| **V11** | **10.0 min** on the gallery: exactly `{"/api/v1/telemetry":2,"/api/v1/cache":2}` — each at its 5-minute interval — and 300 pushed `stats` frames (the 2 s cadence). Nothing else. |
| **V11 (memory)** | Production bundle, 5 rounds × 13 screens. Round 1 loads every lazily-split chunk; **rounds 2→5, 52 further navigations, moved the heap 33.4 KiB** (1,845,636 → 1,879,876 B). Flat. |
| **V11a** | 8 hide-while-navigating cycles: **0** events sockets opened while suspended, **0** left open, server-side ESTABLISHED once settled **0**. |
| **V11b** | `/telemetry` changed in tab B; tab A's selector shows the new value without a reload. |
| **V12** | Initial load: **5 requests** — document, `index` chunk, stylesheet, sprite, `/health`. **No chart chunk fetched**, and none exists to fetch. |
| **V13** | Both themes at 1400 / 900 / 400 px: `scrollWidth == clientWidth` at every combination — no horizontal body scroll. Light `bg rgb(238,241,245)` / text `rgb(31,39,51)`; dark `bg rgb(15,21,29)` / text `rgb(227,233,240)`. **No theme flash**: with `dark` stored and a *light* system preference, the root read `dark` at `DOMContentLoaded`. |
| **V14** | Gate proven to fail once: a 400 KiB incompressible asset produced `postbuild: over budget by 261,424 B gzip` and exit 1. Final figures in the table above. |
| **V15** | `/web` layer 164 kB; **zero** `node` paths in the exported runtime filesystem. |
| **V16** | `fmt --check`, `clippy --workspace --all-targets -D warnings`, `test --workspace` (1,195 passed) — all green. |

Screenshots taken at 1400 / 900 / 400 px in both themes, plus the phone drawer
and the login screen, and compared against `Main`, `MobileNav` and the sidebar
the other artboards draw.

### Two defects found against the artboards, and fixed

Both were found by comparing screenshots against `MobileNav.dc.html`, not by
reading the plan:

1. **The drawer had no footer.** `MobileNav` puts connection state, version,
   Theme and Sign out in a drawer footer because the phone top bar drops them.
   The first implementation hid them from the top bar without adding the footer,
   which left **no way to switch theme or sign out on a phone**. `Sidebar` now
   takes a `footer` slot and the shell fills it; two vitest cases cover it.
2. **The mobile top bar aligned the title right.** With the right-hand group
   hidden, `justify-content: space-between` pushed the title away from the
   burger. `MobileNav` draws burger then title, both left.

### One contradiction in the plan, recorded rather than implemented

**§10's transition order is wrong, and §11.1 says so without noticing.** §10
lists `leave` (release) before `enter` (acquire); §11.1 then argues that
"navigating between two routes that both want `stats` must not tear the socket
down and rebuild it. Because the registry is a refcount, the union never reaches
empty during that transition." Those cannot both hold: releasing the outgoing
route's only `stats` reference *does* take the count to zero, and the socket
manager closes on the announcement, not on a later tick.

Implemented as **acquire-then-release**, which is what makes §11.1's stated
guarantee true. A vitest case (`does not rebuild the socket between two routes
that both want stats`) pins it, and it was this test that surfaced the
contradiction. The plan prose should be corrected; that edit is not made here.

### Known limitations and deferred items

- **No `charts` chunk in a production build of this task** — see *Chunking*
  above. p5-06 lands it.
- **`.json` MIME coverage (V2) is untested** because the bundle emits no JSON
  asset. If one ever ships, that row needs re-running.
- **Safari development** goes through the built image, as §6 documents: Safari
  refuses a `Secure` cookie on `http://localhost`. Not worked around.
- **The event-driven checks were verified through the dev gallery**, not through
  a shipped route, because no shipped p5-05 route needs events. The gallery is
  the surface D8 exists to provide, and V7's server-side count is what makes the
  result independent of the browser's own reporting.
- **`Health.dc.html` draws the Diagnostics group collapsed while a diagnostics
  route is active**, where `Memory.dc.html`, `LiveFeed.dc.html` and
  `MobileNav.dc.html` draw it expanded. The implementation follows the majority
  — expanded on `/diagnostics/*`, collapsed elsewhere. `Health.dc.html`'s
  sidebar is written one line per section and looks like a shorthand rather than
  a decision; worth a glance when p5-09 builds that screen.
- **`Upstreams.dc.html`'s Endpoints card writes "updated 2 m ago"** where
  `Health` and `Main` write "2 m ago" in the same card-title-bar position. The
  implementation follows the majority: `header` placement carries "updated", a
  card title bar does not.
- **The phone drawer scrolls its nav list.** At 400 × 800 with the footer
  pinned, the last item sits under the fold and is reached by scrolling the list.
  The artboard frames are drawn taller than a real device on purpose (sketch
  README), so this is not a mismatch, but it is worth confirming on the real
  device in p5-10.
- **The bundle-size and image figures are dev-box measurements.** The RB5009
  validation belongs to p5-10.

### Documentation

No documentation edit is proposed. §23 items 1, 2, 4, 4a and 5 are already
applied and committed; item 3 belongs to p5-06 and item 6 is a p5-10 proposal.
U16 therefore needed no work. The one plan correction identified above (§10's
transition order) is reported here and not applied — the plan file is the task's
own document, and changing it is the owner's call.

---

## Findings

Reviewed against the task file and the plan, from the source and from the test
suite, not from the verification table. `npm run test` re-run during the review:
**19 files, 200 cases, all passing** — the reported figure is accurate.

Evidence classes below: **measured** = reproduced with a throwaway vitest case
during the review (written, run, deleted; the tree is unchanged); **inspected**
= read off the source with the exact path stated; **inferred** = reasoning about
a case neither reproduced nor read directly.

### Critical

None.

### Major

#### M1 — a socket that dies while the tab is hidden never reconnects if the tab returns inside the grace window

`dashboard/frontend/src/events/socket.ts:144-148`

| | |
| - | - |
| Rationale | `setSuspended(false)` returns early whenever a grace close is armed. That branch assumes the connection is still alive — true only if nothing closed it since `hidden`. If the transport dies during the grace window, `handleClose` moves the manager to `backoff` and, being suspended, starts **no** backoff timer (`socket.ts:313-323`); the grace timer is never cancelled by that path. Becoming visible then cancels the grace and returns, leaving `connection === null`, `connectionState === 'backoff'`, and no timer armed. |
| Failure mode | Phone locks or the app is switched while a route holds `stats`/`query`; Wi-Fi power-save drops the TCP connection; the operator returns within 30 s. The indicator reads `reconnecting` for ever and nothing retries. Recovery needs a route with an **empty** union (which resets the state to `closed`), so navigating Dashboard → Lists does not recover it either — the union stays non-empty and the state stays `backoff`. |
| Evidence | **Measured.** Throwaway case: acquire `stats`, open, `setSuspended(true)`, `onclose()` after 5 s, `setSuspended(false)`, advance 300 s → `state()` is `'backoff'`, one socket ever opened. Expected `'open'`, 2 sockets. |
| Why the suite misses it | `socket.test.ts:342` hides and shows with the connection **alive**; `socket.test.ts:392` enters `backoff` **before** suspending, so no grace is armed. The intersection of the two is untested. |
| Smallest fix | In `setSuspended(false)`, cancel the grace unconditionally and fall through to the existing reconnect check — `this.cancelGrace();` in place of the early-return block. With a live connection the state is `open`, so the `closed \|\| backoff` guard already declines to reconnect and the "no reconnect on a fast hide/show" property is preserved. Add the case above to `socket.test.ts`. |
| Disposition | **Fix before `DONE`.** Not reachable from a shipped p5-05 screen — no production route opens a socket — but it is live the moment p5-06 lands the Dashboard, and it is four lines. |

#### M2 — the DEV route assertion fires on any transition between two lazily-loaded pages

`dashboard/frontend/src/lifecycle/route-lifecycle.ts:104-112`, `src/shell/shell.tsx:78-98`

| | |
| - | - |
| Rationale | The assertion compares the incoming route's declared endpoints against `refresh.subscriberCount(endpoint) > 0` — the registry's **global** count. But widgets subscribe too, through `useRefresh` (`components/refresh-cluster.tsx:36`), and the outgoing page is still mounted when `enter()` runs: the shell renders `<Page route={route}/>` with the *previous* `Page` state, and `setPage(null)` happens in a `useEffect` (`shell.tsx:84-98`) that runs **after** the `useLayoutEffect` holding the transition (`shell.tsx:78-82`). When the element type does not change, Preact diffs the outgoing component in place — it is not unmounted at all during that commit. |
| Failure mode | The assertion throws from inside a layout effect. There is no error boundary, so the render loop aborts. Reachable in p5-05 dev today: `/dev/gallery` → `/settings` (both have a non-`null` `load`) throws `route lifecycle: telemetry subscribed=true declared=false`. From p5-06 on it is the normal case for any built-page → built-page navigation with differing endpoints, so the likely response is to delete the net rather than fix it. |
| Evidence | **Inspected**, path stated above. A jsdom reproduction of the hook ordering was attempted and abandoned as harness noise; the claim does not depend on effect ordering subtleties — the outgoing component simply has not been swapped out yet. |
| Why the suite misses it | `route-lifecycle.test.ts:192` drives `enter()` directly and *asserts* that a foreign subscriber is reported. Nothing exercises the shell's actual transition, where a foreign subscriber is the norm rather than a leak. |
| Smallest fix | Assert against the lifecycle's own acquisitions, not the registry's global count: track the endpoints `enter()` currently holds and compare that set to the declaration. That still catches the leak the net exists for (an endpoint the transition failed to release) and stops indicting widgets that are merely one render behind. |
| Disposition | **Fix before `DONE`.** The net is one of the plan's two stated defences for the phase invariant (§10, §24); leaving it firing spuriously means it will not survive p5-06. |

### Minor

| # | Where | Finding | Impact | Disposition |
| - | ----- | ------- | ------ | ----------- |
| m1 | `src/api/types.ts:121` | `Telemetry.memory` is typed `Record<string, number>`. `MemoryResponse` (`crates/fah-api/src/wire.rs:987`) carries a nested `components` object and four `Option<u64>` fields (`process_rss`, `process_peak_rss`, `major_page_faults`, `minor_page_faults`, `process_rss_anon`, `process_rss_file`). | Nothing reads it in p5-05. A p5-09 Memory screen typed against this gets `number` where `null` or an object ships — a `.toFixed()` on `null` throws at runtime with no type error. Hand-writing the types is the mechanism for reading the contract carefully; this one field was not read. | Fix now (type-only, no behaviour) or explicitly hand to p5-09. |
| m2 | `src/shell/shell.tsx:84-98` | `route.load()` has no `.catch`. | A hashed chunk that 404s — an open tab after a redeploy, the ordinary case for this deployment model — leaves the page on `<div class="boot"/>` for ever, plus an unhandled rejection. `error-state.tsx` exists and is unused here. | Fix before `DONE` — one `.catch` and an `ErrorState`. |
| m3 | `src/styles/layout.css:350-366`, `src/shell/sidebar.tsx` | The phone drawer has no focus trap, no `Escape` handler, and the closed drawer is hidden by `transform: translateX(-100%)` alone. | Plan §15 requires "the drawer focus-trapped while open". As built, thirteen off-screen links stay in the tab order and the accessibility tree at < 768 px whether the drawer is open or not, and an opened drawer does not take focus. `confirm-dialog.tsx:32-57` already has the trap that should have been reused. | Fix before `DONE` or record as a deliberate deferral to p5-10; do not leave the plan claiming a trap that is absent. |
| m4 | `src/components/chart.tsx:58` | The effect keys on `[data, options, height]` by identity. | A caller passing an inline `options` object — the natural way to write it — destroys and rebuilds the uPlot instance on every render. This is the wrapper twelve screens inherit, so the cost compounds. Prefer `setData()` when only `data` changed, and document that `options` must be stable. | Defer to p5-06, which lands the first real chart, but record the contract now. |
| m5 | `src/shell/shell.tsx:56` | `subscribeVisibility` does not deliver the current state at subscribe time, and `RouteLifecycle.setSuspended` starts at `false`. | A tab opened or session-restored in the background polls its route's endpoints and opens the socket until the first `visibilitychange`. Small, but it is the invariant the phase is measured against. | Fix now (one `listener(isHidden())` on subscribe) or defer explicitly. |
| m6 | `src/events/subscriptions.ts:10-12` | The class doc says "Release-before-acquire during a route transition is deliberate". `route-lifecycle.ts:40-50` implements and documents the opposite, correctly. | Two contradicting comments about the one ordering the Implementation Summary calls out as an intentional deviation. Whoever writes p5-06's page reads whichever they find first. | Fix before `DONE` — delete or correct the stale comment. |
| m7 | `src/refresh/registry.ts:202,213` | A manual Refresh that joins an in-flight background request returns early at `if (slot.inFlight !== null)` and never applies `restartTimer`. | Plan §12.3 says a manual refresh restarts the background timer on success. In the join case it does not, so a scheduled fetch can follow the manual one seconds later — the exact thing the rule exists to prevent. Narrow window (only while a background fetch is already in flight). | Defer, recorded. |
| m8 | `src/components/chart.tsx:3` | `import 'uplot/dist/uPlot.min.css'` is static, and `build.cssCodeSplit` is `false`. | The uPlot **JS** split is real and proven. From p5-06 on, when a shipped page imports `Chart`, uPlot's stylesheet joins the single global stylesheet fetched on the login path. "Login must not pull uPlot" will hold for JS only. ~1 KB gzip against a 150 KB budget, so this is a claim-accuracy issue rather than a weight one. | Record; p5-06 decides whether to move it into the lazy path. |
| m9 | repository | `.playwright-mcp/` is untracked at the repo root and absent from `.gitignore`; `dashboard/` is untracked in full. | Plan §23 item 6 states "p5-05 introduces no Playwright". A `git add -A` commits browser-verification scratch into the tree. | Add the ignore entry (or delete the directory) before the commit. |

### Nitpicks

| Where | Note |
| ----- | ---- |
| `src/api/core.ts:84-88` | `parseRetryAfter` accepts delta-seconds only. An HTTP-date `Retry-After` yields `null`, which for a `503` means "never retryable" — a safe direction, but the rule reads as "the API is not serving TLS" on the login page for a case that is not that. |
| `src/shell/topbar.tsx:33` + `src/shell/shell.tsx:123` | Two `ConnectionIndicator`s with `aria-live="polite"` are in the DOM at once; exactly one is `display: none` per breakpoint, so nothing double-announces today. The pairing is fragile — one CSS change makes it a duplicate announcement. |
| `src/shell/sidebar.tsx:94` | The Diagnostics parent carries `aria-current="true"` while the active child carries `aria-current="page"` — two current markers in one subtree. |
| `src/events/socket.ts:231-236` | `onerror` is treated as a close and the handlers are detached, but `close()` is never called on the underlying socket. The WebSocket spec always follows `error` with `close`, so nothing leaks in practice. |
| `src/theme/theme.ts:63` | `subscribeTheme` has no subscribers — theming is CSS-driven. Dead export. |
| `src/lifecycle/timers.ts:42-56` | The 30 s age ticker keeps running while the document is hidden. No network cost; a re-render every 30 s in a background tab. |
| `tsconfig.json` | `include` names `vitest.config.ts`, which does not exist (the config lives in `vite.config.ts`). |

### Intentional decisions — verified, keep as they are

| Decision | Verdict |
| -------- | ------- |
| **Acquire-then-release** during a route transition | **Correct, and the plan is the thing that is wrong.** Verified in `route-lifecycle.ts:51-72` and pinned by `route-lifecycle.test.ts:148`. It also matters for the refresh registry, not only the socket: releasing first would take the endpoint refcount to zero, clear the timer and `abort()` the in-flight request between two routes that both read `/telemetry`. §10's prose should be corrected; see m6 for the stale comment left behind. |
| **Three modules beyond §1's list** (`lifecycle/route-lifecycle.ts`, `services.ts`, `shell/icon.tsx`) | Justified. The lifecycle extraction is what makes the transition testable without a browser (16 cases). `services.ts` is the only thing preventing a re-render from constructing a second `SocketManager`. `icon.tsx` confines the hashed sprite URL to one module. No other file was added — `theme/theme.ts` and `styles/layout.css` were both in §1. |
| **No `charts` chunk in a production build** | Verified: `components/chart.tsx` is imported only by `pages/dev-gallery.tsx`, which is reachable only through `GALLERY_ROUTE.load`, guarded by `import.meta.env.DEV`. Emitting a chunk no shipped page fetches would be weight for nothing. The split mechanism is real (`await import('uplot')`) and grep-asserted. See m8 for the one part of the claim that will not survive p5-06 unchanged. |
| **Deferrals to p5-06 / p5-10** (page cache, restart-required banner, IA §Dashboard sentence, endpoint-cost measurement, RB5009 figures) | All four are deferred against a stated reason rather than an omission. The page cache in particular: `PAGE_CACHE_CAPACITY = 4` was a number picked, not derived, and there is no payload to cache. Correct call. |
| `built` flag on the route table | Keeps §9's table complete without a second, driftable copy of the mapping, and `effectiveEvents`/`effectiveEndpoints` are the only readers. |
| Never `{"subscribe":[]}` | Enforced at `socket.ts:177-191` and asserted. |
| No client-side frame-size validation, no outbound protocol abstraction | Both unreachable/speculative as argued. Agreed. |
| Hand-rolled router, own postbuild script, native `<select>` | Each buys a real property (lifecycle certainty, one pass that both compresses and gates, free keyboard/mobile behaviour) at zero dependency weight. |
| One `RefreshCluster` per polled endpoint, request-triggering confined to it | This is what makes route-scoped fetching auditable rather than asserted. Keep the rule explicit for p5-06. |

### On whether the tests prove the guarantees

They are unusually good for a foundation task and they do more than the happy
path — the probe cooldown, the `OPEN_STABLE_MS` reset boundary, "retained value
is not a subscription", "no request on a selector change", and the `setInterval`
grep are all adversarial cases. Two structural gaps:

| Gap | Consequence |
| --- | ----------- |
| Suspension and connection failure are tested **separately** | M1. Every visibility case keeps the socket healthy; every failure case starts from a visible tab. |
| Nothing exercises the shell's real route transition | M2. `route-lifecycle.test.ts` drives `enter()` directly, so the one thing the shell adds — a page that is still mounted when the transition runs — is never seen. One jsdom case mounting `Shell` across a route change would have caught it. |

Everything else the task lists as an acceptance criterion is covered either by a
vitest case or by a V-row read off a running container, and the server-side
connection count in V7/V11a is the right instrument for the claim it supports.

---

## Fixes applied

Approved by the owner after the review above. No invariant was weakened to make
a finding go away: the two Major fixes make the socket's single-owner rule and
the route-scoped net hold in cases where they previously did not, and every
behavioural change carries a regression case.

### One correction to the review itself

**m1 was partly wrong as written.** `MemoryComponentsResponse` is
`#[serde(flatten)]`ed into `MemoryResponse` (`crates/fah-api/src/wire.rs:988`),
so `memory` arrives as **one flat object**, not with a nested `components`
block. The nullability half of the finding stands and is what the fix addresses
— seven of its thirteen fields are `Option<u64>`.

### What changed

| Finding | Action | Files | Regression coverage |
| ------- | ------ | ----- | ------------------- |
| **M1** — socket stranded after a hidden-window transport death | **Fixed.** `setSuspended(false)` now cancels the pending grace close and falls through to the reconnect check instead of returning on the strength of an armed timer. The existing `closed \|\| backoff` guard is what keeps a live connection from being rebuilt, so "no reconnect on a fast hide/show" is unchanged. | `events/socket.ts:144-150` | `socket.test.ts` — *reconnects when the transport died inside the grace and the tab came back*. Fails against the old code with `state = 'backoff'`, one socket. |
| **M2** — DEV assertion indicted every built-page → built-page navigation | **Fixed.** The assertion now asks what **this transition** holds, via a refcount kept as it acquires and releases, instead of reading the registry's global subscriber count. A widget on the not-yet-unmounted outgoing page no longer trips it; a release that never ran, or ran twice, still does. | `lifecycle/route-lifecycle.ts:26-35,55-80,95-135` | `route-lifecycle.test.ts` — three cases: *reports an endpoint the transition failed to release*, *reports an endpoint released twice*, *does not indict a widget of the outgoing page that has yet to unmount*. The last one fails against the old code. |
| **m1** — `Telemetry.memory` typed `Record<string, number>` | **Fixed.** Replaced with a named `Memory` interface: thirteen fields, seven of them `number \| null`, matching `MemoryResponse` field for field. | `api/types.ts:121-146` | Type-only; `npm run typecheck` is the check. |
| **m2** — chunk-load failure left the route on the boot placeholder | **Fixed.** `route.load()` gains a `.catch` that renders `ErrorState` with a message naming the cause (the UI and API deploy as one artifact, so a 404 on a hashed chunk means the tab outlived its build). Covers the login route too. | `shell/shell.tsx:31-35,50,105-124,132-141` | None — a rejecting dynamic import in a mounted `Shell` needs a jsdom harness for the whole shell, which does not exist and is not worth building for one `.catch`. Verified by inspection. |
| **m3** — no drawer focus trap; closed drawer still focusable | **Fixed.** The trap was extracted from `confirm-dialog.tsx` into `components/focus-trap.ts` and is now used by both, so the Tab arithmetic exists once. The shell traps the drawer while open, `Escape` closes it, focus returns to the burger. Below 768 px the closed drawer is `visibility: hidden` (transitioned, so the close animation still plays), which takes its thirteen links out of the tab order and the accessibility tree. | `components/focus-trap.ts` (new), `components/confirm-dialog.tsx`, `shell/shell.tsx:100-110`, `styles/layout.css:350-372` | `focus-trap.test.tsx` — four cases: focus moves in, Tab and Shift+Tab cycle at the ends, `Escape` asks the owner to close and focus returns to the invoker, an inactive trap adds no listener. |
| **m4** — `Chart` rebuilt the plot on every render | **Fixed.** The instance is now keyed on `[options, height]`; new readings go through `setData` in their own effect. `data` is read from a ref at creation, since creation is one dynamic import later than the render that asked for it. `ChartProps.options` documents that it must be referentially stable. | `components/chart.tsx:5-20,30-35,48,58-73` | None runtime — uPlot needs a canvas 2D context that jsdom does not implement. **p5-06 lands the first real chart and is where this gets exercised**; recorded so it is not assumed proven. |
| **m5** — visibility state not read at mount | **Fixed.** `subscribeVisibility` delivers the current state to a new subscriber before returning. A tab opened or session-restored in the background is now suspended from the start rather than at its first `visibilitychange` — which for a tab never brought forward never arrives. | `lifecycle/visibility.ts:27-42` | `visibility.test.ts` — *reports the current state at subscribe time*, and the transition case updated to the new first value. |
| **m6** — stale ordering comment | **Fixed.** `subscriptions.ts`'s class doc now states acquire-then-release and points at the module that performs it. | `events/subscriptions.ts:5-17` | n/a. |
| **m7** — a Refresh that joined a background fetch skipped the timer reset | **Fixed.** The reset is now a flag on the slot rather than an argument to the call that happened to start the request, so a joining click keeps it. Cleared in `finally`, on failure too: a failed refresh leaves the timer untouched and must not arm the next background fetch. | `refresh/registry.ts:29-45,163-172,207-222,236-243` | `registry.test.ts` — *restarts the background timer even when the click joined a background fetch* and *leaves the timer alone when the joined request failed*. |
| **m8** — uPlot's stylesheet is a static import | **Deferred to p5-06**, with the reason recorded rather than the finding closed. `cssCodeSplit: false` merges every stylesheet into one file regardless of how the CSS is imported, so moving the import into the dynamic path today changes nothing that ships — and nothing ships, since `chart.tsx` is not in the production graph. p5-06 lands the first real chart and is the task that can measure the ~1 KB and decide between inlining it and splitting the chart chunk's CSS. | — | — |
| **m9** — `.playwright-mcp/` untracked and unignored | **Fixed.** `.gitignore` gains the entry, with a comment stating that p5-05 ships no Playwright and this only keeps a verification run out of the tree. | `.gitignore` | n/a. |

### Nitpicks

| Nitpick | Action |
| ------- | ------ |
| Diagnostics parent carried `aria-current="true"` beside a child's `"page"` | **Fixed** — the parent carries none; the active child answers "where am I" once. |
| `subscribeTheme` had no subscribers | **Removed**, with the listener set and its test (principle 14). Theming is one attribute on `<html>` and every token is CSS, so nothing re-renders and nothing needs telling. |
| `tsconfig.json` named a non-existent `vitest.config.ts` | **Fixed.** |
| Two `aria-live` connection indicators in the DOM | **Not a defect — no change.** Exactly one is `display: none` per breakpoint, and a `display: none` live region is not announced. Making one of them non-live would silence the announcement at the other breakpoint, which is worse. The pairing is recorded so a future CSS change does not create a duplicate silently. |
| `parseRetryAfter` accepts delta-seconds only | **Deferred, no target task.** `fah-api` emits seconds everywhere `Retry-After` appears; an HTTP-date form would parse to `null`, which is the safe direction (no retry timer). Adding date parsing is code against a case the server does not produce. |
| `onerror` detaches handlers without calling `close()` | **Accepted, no change.** The WebSocket spec fires `close` after `error` in every "fail the connection" path, so the underlying socket does terminate. Adding a `close()` would be belt-and-braces against a case the platform rules out. |
| Age ticker runs while the document is hidden | **Accepted, no change.** No network cost and no route work — one re-render every 30 s in a background tab. Suspending it would put a second visibility consumer in play for a presentation concern, which is the coupling §13.1 removed on purpose. |

### One module added beyond §1's file list, making four

`components/focus-trap.ts` — extracted rather than written: `confirm-dialog.tsx`
already held a trap, and m3's fix needed the same one for the drawer. Two copies
of the Tab arithmetic is two places for it to be subtly wrong (principle 4). It
schedules nothing, so the timer-ownership grep is unaffected.

### Gates after the fixes

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | **209 passed, 20 files** (was 200 / 19: +11 new cases, −2 removed with `subscribeTheme` and the superseded assertion case) |
| `npm run build` | succeeds, gate not tripped |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | **1,195 passed**, 8 ignored, 44 suites |

**Bundle after the fixes** — three chunks unchanged, no uPlot, no charts chunk:

| file | raw | gzip | brotli |
| ---- | ---: | ---: | ---: |
| `assets/index-*.js` | 38,711 | 13,663 | 12,358 |
| `assets/style-*.css` | 14,293 | 3,725 | 3,244 |
| `assets/sprite-*.svg` | 3,733 | 858 | 750 |
| `assets/login-*.js` | 1,852 | 936 | 805 |
| `index.html` | 1,130 | 611 | 430 |
| `favicon.svg` | 258 | 185 | 163 |
| `assets/system-*.js` | 135 | 130 | 90 |
| **TOTAL** | **60,112** | **20,108** | **17,840** |

**20,108 B gzip against the 153,600 B budget — 13.1 %**, up 761 B from 19,347 B.
The increase is the focus trap, the chunk-failure path and the drawer CSS.

### Re-verification not repeated

The V1–V16 table was taken against a running container before these fixes. The
fixes touch the socket's resume path, the DEV-only assertion, one type, one
error path, the drawer's focus and CSS, the chart's effect keys, the visibility
subscribe path, one comment, and the refresh timer-reset flag. **V13 (both
themes, three breakpoints, no horizontal scroll) and V10/V11a (backgrounding)
are the two rows whose subject moved**, and neither was re-measured in a
browser: the drawer change is CSS plus a keyboard trap, and the socket change is
covered by a unit case that reproduces the defect. Both belong in p5-10's
device pass, which already re-runs the phone drawer against real hardware.

---

## Post-fix re-review — the changed hunks only

Scope was the fix diff, not the project. `dashboard/` is untracked, so there is
no git baseline for it; the eighteen touched files were re-read at the changed
ranges instead. Nothing new was found, and no fix introduced a second defect.

| Checked | Result |
| ------- | ------ |
| `socket.ts` resume path | `cancelGrace()` then the unchanged `closed \|\| backoff` guard. A live connection is in `open`, so the guard still declines — the fast hide/show property is preserved by the guard, not by the removed early return. |
| `route-lifecycle.ts` refcount | `held` is incremented at acquisition and decremented inside the release closure, so the assertion reads bookkeeping rather than a copy of the declaration. The union check is untouched and still global, correctly — the subscription registry has exactly one caller. |
| `shell.tsx` | `loadFailed` is reset at the top of the load effect, so a failure does not persist across a later navigation. Both render paths (login and shell body) handle it. The trap's cleanup returns focus to the burger when navigation closes the drawer — the right landing place, not a lost focus. |
| `chart.tsx` | Instance keyed on `[options, height]`; `data` reaches creation through a ref because creation is one dynamic import later than the render. The `setData` effect is a no-op before the instance exists, and the first paint carries the newest data either way. |
| `registry.ts` | The reset flag is cleared in `finally`, so a failed or aborted request cannot arm the next background fetch. |
| `focus-trap.ts` | Keys on `active` alone; `resolve`/`onEscape` go through a ref, so an inline arrow at the call site cannot rebind the listener and re-steal focus every render. Both call sites pass inline arrows. |
| `visibility.ts` | The synchronous first delivery lands before the returned unsubscribe exists, which is what the shell wants: `setSuspended` is idempotent and starts `false`, so a visible tab sees a no-op. |
| Timer ownership | `focus-trap.ts` schedules nothing; the grep test covers the new file and passes. |

### Working tree — confirmed against intent

| Path | State | Intended |
| ---- | ----- | -------- |
| `.gitignore` | modified, +9 | yes — `dashboard/frontend/node_modules/`, `dashboard/frontend/dist/`, `.playwright-mcp/` |
| `Dockerfile` | modified, −28/+16 | yes — `frontend` stage builds the real bundle; `COPY --from=frontend /web /web` untouched |
| `dashboard/.gitkeep` | deleted (staged) | yes — the directory is no longer empty |
| `dashboard/` | untracked, **83 files** | yes — **zero** paths matching `node_modules` or `/dist/`; only `index.html`, `package.json`, `package-lock.json`, `tsconfig.json`, `vite.config.ts`, `public/`, `scripts/`, `src/` |
| `docs/code-review/phase5/p5-05-frontend-foundation-review.md` | untracked | yes — this file |
| `.playwright-mcp/` | **no longer listed** | yes — the new ignore entry works |

**One unrelated modification is in the tree and must not ride along.**
`docs/code-review/phase2.6/p2.6-11-optin-deploy-soak-review.md` carries a +48
line *Interim reading — day 1 of 7* from the parallel 2.6 soak track. It belongs
to `phase2.6-adaptive-stage1`, not to p5-05, and the phase `CLAUDE.md` §Parallel
track keeps the two apart. **Stage p5-05's paths explicitly; do not `git add -A`.**

### Gates re-run after every fix

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | 209 passed, 20 files |
| `npm run build` | succeeds; 20,108 B gzip / 17,840 B brotli, **13.1 %** of the 153,600 B budget |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | 1,195 passed, 8 ignored, 44 suites |

---

## Status

**PASS WITH DEFERRED FINDINGS.**

Both blockers are fixed with regression coverage that fails against the previous
code. Seven of the nine Minors are fixed; **m8** is deferred to **p5-06** with a
stated reason (nothing it would change ships today), and **m4** is fixed but
its runtime proof is deferred to **p5-06**, which lands the first real chart.
Three Nitpicks are fixed, one is answered as not-a-defect, and three are
accepted with the reason recorded.

The architecture was not changed. Both Major findings were failures to hold an
existing boundary at one edge — the socket manager's single-owner rule on the
resume path, and the route-scoped net's idea of whose subscriptions it is
counting — and both fixes make the boundary hold rather than relax it.
