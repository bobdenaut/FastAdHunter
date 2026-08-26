# P5-05 — Implementation Plan: Frontend Foundation

**Task:** [p5-05-frontend-foundation.md](p5-05-frontend-foundation.md) ·
**Phase:** 5 · **Depends on:** p5-01, p5-03, p5-04 · **Model:** Opus ·
**Branch:** `phase5-05` (from the completed `phase5-04`)

Written against: [visual-system.md](../../../docs/dashboard/visual-system.md),
[information-architecture.md](../../../docs/dashboard/information-architecture.md),
[API.md](../../../API.md) §Events / §Session authentication / §Error format,
`crates/fah-api/src/web.rs`, `Dockerfile`, and the p5-01 / p5-03 / p5-04
Implementation Summaries.

---

## The binding constraint

**The final product looks exactly like the sketch.**
[docs/dashboard/sketch/](../../../docs/dashboard/sketch/) is not a mood board, a
suggestion or a starting point — it is the specification for layout, chrome,
density, control placement and interaction, and the built UI matches it screen
for screen. Phase `CLAUDE.md` rule 8 already makes it binding; this plan states
it again because this task builds the shell and the component vocabulary every
later screen is assembled from, so a drift introduced here is inherited
thirteen times.

Where this document and an artboard disagree, **the artboard wins** and this
document is wrong and gets fixed. Where the artboard is silent, visual-system.md
decides. Where both are silent, ask — do not invent chrome. The refresh cluster
in §13.1 is the worked example: it was specified in prose first, the prose
invented a control family the artboards do not have, and the drawing was
corrected before a line of code was written.

Nothing here licenses importing from the sketch. It is read, matched and kept
current — never built from.

---

## 0. Decisions closed by the owner

Settled before implementation. Nothing below is re-decided during the work.

| # | Decision | Reasoning |
| - | -------- | --------- |
| D1 | **Hand-rolled router**, ~40 lines over `history.pushState` + `popstate` | The route-scoped lifecycle is the invariant this task is measured against. A library's mount/unmount timing has to be understood exactly as well as an own one, so the dependency buys markup sugar and no lifecycle certainty. 13 fixed paths, no path parameters in this task. |
| D2 | **One own post-build Node script** does compression *and* the size gate | `node:zlib` ships brotli and gzip. The pass that compresses is the pass that measures, so the reported figure and the gated figure cannot diverge. Zero dependency; mirrors the pattern already proven in the p5-01 fixture stage. |
| D3 | **Budget denominator = every shipped byte except the `.gz` / `.br` siblings**, each gzipped, summed | visual-system.md says "under 150 KB gzip, all in". Lazily-loaded chunks count, or the split becomes a way to hide weight. Brotli sum reported beside it. |
| D4 | **vitest**, devDependency only; `npm run test` joins the phase gates | Zero runtime bytes. The invariant lives in refcount arithmetic — subscription union, refresh subscriber counts, backoff, probe classification — which is far cheaper to pin in unit tests than in a browser, and five later tasks reuse the suite. |
| D5 | **Vite dev server on `http://localhost:5173` proxying to the TLS API** | `POST /auth/login` answers `503` when `api.tls = false`, and the cookie is `__Host-` / `Secure`, so development must talk to the TLS listener. Chrome and Firefox accept a `Secure` cookie on `http://localhost`; **Safari does not** — documented, not worked around. See §6 for the `Origin` trap, which is load-bearing. |
| D6 | **Timing constants as proposed** (§17) | Hidden-close grace 30 s · backoff 1·2·4·8·15·30 s with ±20 % jitter · probe after 3 immediate failures. One module holds all of them. |
| D10 | **Background refresh is slow and per-endpoint** — `/health` 60 s, `/telemetry` 300 s, `/cache` 300 s — with an explicit **manual Refresh** on every endpoint-backed card | A dashboard left open has no reason to re-read a whole-engine snapshot every ten seconds. The interval's job shrinks to "never silently hours stale"; "I want it now" is the button. The three figures and the option sets are **provisional** — nobody has measured what `/telemetry` or `/cache` cost on the RB5009, and p5-10 corrects them from evidence (§23 item 5). `stats` is unaffected: 2 s push, never polled. |
| D11 | **The interval selector lives with the data**, not in Settings — **once per polled endpoint on a page** | The operator changes refresh behaviour where the data is being viewed; leaving the page to change a number and coming back is the worse design, and Settings does not exist until p5-09 anyway. One per *endpoint*, not per card: the preference is global, so five cards on `/cache` drawing five identical dropdowns is five copies of one control. Placement is settled by the artboards, not by this table — see §13.1. |
| D12 | **The preference is global per endpoint and browser-local** | Changing the `/telemetry` selector on any card changes it for every card and every page reading `/telemetry`. Intentional: it is one operator's viewing preference for one browser, not per-widget state and not a server setting. A config key would make one household member's taste global to every client and add a CONFIGURATION.md section for a browser preference. |
| D13 | **Bounded option sets, no free text, no `Off`** — `/health` 30 / 60 / 300 s · `/telemetry` 60 / 300 s · `/cache` 60 / 300 s | A typed value lets the UI degrade the resolver. The floors differ because the costs differ by an unknown factor, and `/telemetry` and `/cache` do not get 30 s until something has measured them. No `Off`: a card that never refreshes and never says so is the failure this whole change exists to avoid, and Refresh covers the case anyway. |
| D14 | **Data age travels with the cluster**, so every polled endpoint on a page shows its age exactly once | At a 5-minute interval an unlabelled figure is a lie. It is also already required: IA's "windows are labelled wherever two of them meet" rule bites the moment 2-second-fresh push tiles sit beside five-minute-old `/telemetry` cards on the Dashboard. |
| D15 | **The sketch artboards are the layout specification and were updated in this change** | `Cache`, `Upstreams`, `Health`, `Main` and `MobileDashboard` now draw the cluster. Phase `CLAUDE.md` rule 8 makes the sketch binding for layout and interaction, and its README says a drifted sketch is worse than none — so the drawing moved first and this plan describes what is drawn. |
| D7 | **Sidebar renders all 13 entries**; an unbuilt route resolves to the empty-state component | The four-section chrome is a deliverable of this task and has to be seen in both themes at three breakpoints. No fake figures, so phase rule 1 holds. Those routes declare **no** event types and **no** endpoints, because they render nothing — so the indicator correctly reads `not needed here` on every shipped route. |
| D8 | **Dev-only component gallery** at `/dev/gallery`, excluded from the production build and asserted absent | Production ships no route that opens a socket, which is correct while no shipped page renders events. The gallery is what exercises the socket, the subscription re-send, the probe and the refresh refcount against a live API via `npm run dev`. |
| D9 | **information-architecture.md's stale baseline sentence is corrected in this change**, proposed and approved before it lands | Route-scoped is the later decision and it is in the phase `CLAUDE.md`. See §22. |

---

## 1. Shape

One new directory, one Dockerfile stage rewritten, two repository files touched.
No Rust source change, no new API route, no config key.

```text
dashboard/frontend/
  package.json  package-lock.json  tsconfig.json  vite.config.ts  index.html
  scripts/postbuild.mjs
  public/favicon.svg
  src/
    main.tsx  app.tsx  constants.ts
    api/       core.ts  types.ts  auth.ts  health.ts  telemetry.ts  cache.ts  index.ts
    events/    socket.ts  subscriptions.ts  backoff.ts  probe.ts  types.ts
    refresh/   registry.ts  preferences.ts  use-refresh.ts
    router/    router.ts  routes.ts  link.tsx
    lifecycle/ visibility.ts  timers.ts
    session/   session.ts  guard.tsx
    shell/     shell.tsx  sidebar.tsx  topbar.tsx  content-header.tsx
               connection-indicator.tsx
    components/ tile.tsx  card.tsx  table.tsx  verdict-pill.tsx  status-pill.tsx
                stage-bar.tsx  empty-state.tsx  error-state.tsx
                confirm-dialog.tsx  chart.tsx
                refresh-cluster.tsx  data-age.tsx
    pages/     login.tsx  not-yet-built.tsx  dev-gallery.tsx
    theme/     theme.ts
    styles/    tokens.css  base.css  layout.css  components.css
    assets/    sprite.svg
```

Tests are colocated as `*.test.ts` beside the module they cover.

**The no-comments rule does not apply here.**
`.claude/hooks/no-rust-comments.sh` matches `*.rs` only. TypeScript may carry
comments; write them where the *why* is not in the code, per the ordinary
standard, not per hard rule 7.

---

## 2. Toolchain

| Item | Value |
| ---- | ----- |
| Node | 22 — the Dockerfile pins `node:22.21.1-alpine`; `engines.node` is `">=22"` |
| Package manager | npm, `package-lock.json` committed, image builds with `npm ci` |
| Runtime dependencies | `preact`, `uplot` — nothing else |
| Dev dependencies | `vite`, `typescript`, `@preact/preset-vite`, `vitest`, `@types/node` |
| Versions | pinned exactly (no `^`), resolved at implementation time and recorded in the review file with each one's gzip contribution |

**Any dependency beyond that list is a decision, not a convenience.**
visual-system.md is the document it has to be argued against.

`tsconfig.json`: `strict`, `noUncheckedIndexedAccess`, `noImplicitOverride`,
`exactOptionalPropertyTypes`, `verbatimModuleSyntax`, `jsx: "react-jsx"`,
`jsxImportSource: "preact"`, `moduleResolution: "bundler"`, `target: "ES2022"`,
`noEmit: true`.

Scripts:

| Script | Command | Role |
| ------ | ------- | ---- |
| `dev` | `vite` | development, §6 |
| `typecheck` | `tsc --noEmit` | phase gate |
| `test` | `vitest run` | phase gate (D4) |
| `build` | `npm run typecheck && vite build && node scripts/postbuild.mjs` | phase gate; **fails over budget** |
| `preview` | `vite preview` | inspect the real bundle without Docker |

`build` runs the typecheck first on purpose: esbuild strips types without
checking them, so a build that skipped `tsc` would ship code the gate never saw.
The Docker stage runs `npm run build`, so the image build inherits both.

---

## 3. Vite configuration and bundle shape

| Setting | Value | Why |
| ------- | ----- | --- |
| `build.target` | `"es2022"` | explicit, per the task. No `@vitejs/plugin-legacy`. |
| `build.sourcemap` | `false` | maps in the image are weight nobody reads on the device |
| `build.modulePreload` | `false` | **the load-bearing one.** The default injects `<link rel="modulepreload">` for a dynamic import's dependencies, eagerly fetching the very chunks that were split out. On a LAN, one discovery round trip when the lazy route is entered costs less than that. |
| `build.cssCodeSplit` | `false` | one stylesheet for the whole app: fewer requests, no per-chunk flash, and the own-CSS total is small |
| `build.assetsInlineLimit` | `0` | the SVG sprite stays a real hashed file under `/assets/`, not a data URI inside the JS chunk |
| `build.assetsDir` | `"assets"` (default) | p5-01's handler serves `/assets/*` `immutable` and everything else `no-cache`; the emitted layout must match that split exactly |
| `base` | `"/"` | same origin, root-mounted |
| `rollupOptions.output.manualChunks` | **not set** | chunk boundaries come from dynamic imports, which is where they are legible. A `manualChunks` map is a second place for the split to live, and to drift. |

**Chunks — three in this task, four from p5-06 on.**

| Chunk | Contents | Fetched when |
| ----- | -------- | ------------ |
| `index` | shell, router, API core, session, socket manager, shared refresh, component vocabulary, login | first navigation |
| `charts` | `uplot` + `components/chart.tsx` | `await import()` inside the chart wrapper, on first render of a chart |
| `system` | the Settings + Diagnostics route group | lazy route load on first navigation to `/settings` or `/diagnostics/*` |

`system` holds only the not-yet-built page in p5-05 and is still split
deliberately: it proves route-level lazy loading end to end, and p5-09 fills it.
**Login must not pull `uplot`** — asserted (§19), not assumed, because the chart
wrapper is imported by the gallery and one static import anywhere collapses the
split silently.

`index.html` carries the pre-paint theme script (§14) and
`<link rel="icon" href="/favicon.svg">`. That link removes the browser's bare
`/favicon.ico` probe, which would otherwise hit p5-01's SPA fallback and receive
`200 text/html`.

---

## 4. Pre-compression and the size gate — `scripts/postbuild.mjs`

One script, run after `vite build`, over `dist/`.

```text
for every emitted file, skipping *.gz and *.br:
    raw = bytes
    gz  = gzipSync(raw, { level: 9 })
    br  = brotliCompressSync(raw, { QUALITY: 11, MODE: TEXT|GENERIC, SIZE_HINT: raw.length })
    write dist/<file>.gz and dist/<file>.br when smaller than raw
    accumulate raw / gz / br totals
print the per-file table and the three totals
exit 1 when the gzip total exceeds BUDGET_BYTES
```

| Rule | Value |
| ---- | ----- |
| `BUDGET_BYTES` | `150 * 1024` |
| Gated figure | **gzip total**, per D3 — every emitted file except the siblings |
| Reported beside it | brotli total and raw total, per file and summed |
| Sibling written | only when strictly smaller than the source. `ServeDir` selects a sibling whenever it exists, so a larger one would make the served response bigger than the file. |
| Brotli mode | `BROTLI_MODE_TEXT` for `.html` `.js` `.css` `.svg` `.json`, generic otherwise |
| Determinism | no timestamps, no randomness — an unchanged source produces an identical layer, as p5-01's stage does |

The same pass runs three **forbidden-content greps** over the emitted files and
exits non-zero on a hit:

1. the dev gallery's marker string (D8) — a dev-only route that survives
   tree-shaking is exactly what reaches production unnoticed;
2. `http://` or `https://` in the emitted assets — no CDN, no external
   reference, per visual-system.md §Output shape;
3. `pi-hole` / `pihole`, case-insensitive — phase rule 3, including comments,
   alt text, page titles and asset names.

Proving the gate fails is an acceptance item: drop an incompressible file into
`src/assets/`, run `npm run build`, record the non-zero exit and the printed
overage, remove it. The comparison itself is also unit-tested, so the proof does
not depend on remembering to redo it.

---

## 5. Dockerfile and repository plumbing

The `frontend` stage stops emitting a fixture and builds the real bundle.

```dockerfile
FROM --platform=$BUILDPLATFORM node:22.21.1-alpine AS frontend
WORKDIR /app
COPY dashboard/frontend/package.json dashboard/frontend/package-lock.json ./
RUN npm ci
COPY dashboard/frontend/ ./
RUN npm run build && mkdir -p /web && cp -R dist/. /web/
```

| Point | Detail |
| ----- | ------ |
| `COPY --from=frontend /web /web` in the runtime stage | **unchanged, verbatim.** `crates/fah-api/src/web.rs`'s `the_fixed_root_is_the_directory_the_image_ships` test greps the Dockerfile for that exact line. |
| Manifest copied before the source | `npm ci` then caches across every source-only edit |
| `--platform=$BUILDPLATFORM` | kept: the output is architecture-free, and without it the arm64 build runs the whole Node toolchain under QEMU for nothing |
| `ARG FAH_VERSION` in this stage | **removed** — the version now comes from `GET /health` at runtime (§7), so nothing in the bundle needs it. The stage's fixture header comment is rewritten in the same edit. |
| Node in the runtime image | still none. Nothing leaves this stage but the emitted files. |
| `.dockerignore` | already correct from p5-01 (`/dashboard` unblocked, `**/node_modules` and `dashboard/frontend/dist` excluded). No change. |
| `.gitignore` | **add** `dashboard/frontend/node_modules/` and `dashboard/frontend/dist/` |

Image budget: ≤ 30 MB, 13.0 MiB today. The bundle adds under 150 KB gzip of
content plus its `.br` / `.gz` siblings. The measured delta goes in the review
file.

---

## 6. Dev workflow — and the `Origin` trap

`vite.config.ts`:

```ts
server: {
  port: 5173,
  proxy: {
    '/api':    { target: 'https://localhost:8443', secure: false, changeOrigin: true, ws: true,
                 headers: { Origin: 'https://localhost:8443' } },
    '/health': { target: 'https://localhost:8443', secure: false, changeOrigin: true },
  },
}
```

**Why the explicit `Origin` header is not optional.**
`crates/fah-api/src/routes.rs:1545` rejects a cookie-authenticated WebSocket
upgrade whose `Origin` does not match the request's own effective target origin,
and `same_origin()` derives the scheme from `state.tls`, not from the presented
origin — asserted by
`the_scheme_follows_api_tls_rather_than_the_presented_origin`. Through the proxy
the browser sends `Origin: http://localhost:5173`, which fails on both scheme
and port, and `changeOrigin: true` rewrites `Host`, **not** `Origin`. So the
proxy must present `https://localhost:8443` explicitly, matching the `Host` it
sends.

Without that line the socket 401s in development and the failure is
indistinguishable from an expired session: the REST probe (§11.3) classifies it
as one and bounces the developer to `/login` in a loop. This is the single most
expensive way to lose an afternoon on this task.

| Fact | Consequence |
| ---- | ----------- |
| `__Host-` requires `Secure` | the API must run with `api.tls = true`; `login` answers `503` otherwise |
| Chrome / Firefox accept `Secure` on `http://localhost` | `npm run dev` works there |
| Safari does not | Safari development goes through `npm run build` and the image. Documented, not worked around. |
| Self-signed certificate | `secure: false` on the proxy; the browser never reaches the API origin directly, so no certificate exception is needed for the dev server |

`vite preview` gets the same proxy block, so the real bundle can be exercised
without rebuilding the image.

---

## 7. Typed API client

`src/api/core.ts` owns one `request()`, and everything else derives from it.

| Element | Contract |
| ------- | -------- |
| Envelope | `{ "error": { "code", "message" } }` on every non-2xx (API.md §Error format) |
| `ApiError` | `class ApiError extends Error { code: string; status: number; retryAfter: number \| null }` |
| Known codes | the documented eight: `bad_request` `unauthorized` `not_found` `conflict` `validation_failed` `rate_limited` `unavailable` `internal` |
| Unknown `code` | kept as a string, never rejected — the compatibility contract permits new fields and codes |
| Unknown fields | ignored. No runtime schema validation. Types describe the documented shape; the response is read, not policed. |
| Credentials | `credentials: 'same-origin'` on every request. The dashboard never sends the bearer key. |
| `Retry-After` | parsed into `retryAfter`, `null` when absent. **A `503` with no `Retry-After` is the `api.tls = false` case and must never be retried on a timer** (API.md §Error format). |
| `401` | handed to the session guard once per request (§8), then rethrown |
| Abort | every call takes an `AbortSignal`; a route unmount aborts what it started |

**Module coverage in this task:** `auth`, `health`, `telemetry`, `cache` — the
endpoints the foundation actually calls. Every later task adds its own resource
module against this same core and **must not** introduce a second fetch wrapper.
Writing the full endpoint surface here would be types nothing reads, checked
against nothing; the contract gets read carefully by the task that consumes it,
which is where a misreading is caught.

**Version in the top bar** comes from `GET /health` (`version`, `status`,
`uptime_seconds`) — unauthenticated, so the login page shows it too. Fetched
once per shell mount, never polled; `/health`'s periodic reads belong to the
shared refresh and happen only while a mounted page asks for them.

---

## 8. Session, login, route guard

There is no session-introspection endpoint. Session state is inferred, and that
is the design:

| Signal | Meaning |
| ------ | ------- |
| `POST /auth/login` → `204` | signed in. The cookie is `HttpOnly` and unreadable from JS, so nothing is stored client-side. |
| Any authenticated call → `401` | session absent or expired → clear in-memory state, navigate to `/login`, keeping the attempted path for a post-login return |
| `POST /auth/logout` → `204` | cookie cleared client-side; the token stays valid until expiry, as API.md states |
| App start | assume signed in, render the shell, let the first `401` correct it. A boot probe would be one request every load for a state the cookie already answers. |

Login page: password field, submit, and the documented failure set rendered from
the envelope —

| Response | Rendering |
| -------- | --------- |
| `401` | wrong password (the two `401` causes are byte-identical by design; the UI must not claim to tell them apart) |
| `429` + `Retry-After` | countdown until the bucket frees |
| `503` **with** `Retry-After` | verification saturated, try again shortly |
| `503` **without** `Retry-After` | "the API is not serving TLS; sign-in stays unavailable until the operator changes that and restarts" — **no retry timer** |
| `400` | malformed request |

No response body exists on success, so nothing is read from one.

The guard is shell-level, not per-page: one `401` handler installed in
`api/core.ts` with the router's navigate. A page never handles `401`.

---

## 9. Router and the route table

`src/router/routes.ts` is the single declarative source. The shell reads it; a
page declares nothing about its own data lifecycle — the task's requirement that
"the shell owns it so no page has to remember".

```ts
type Route = {
  path: string;
  title: string;                 // page title and content-header name
  section: 'overview' | 'filtering' | 'runtime' | 'system';
  group?: 'diagnostics';
  events: EventType[];           // acquired on mount, released on unmount
  endpoints: RefreshEndpoint[];
  load: () => ComponentType | Promise<ComponentType>;
};
```

| Path | Screen | `events` | `endpoints` |
| ---- | ------ | -------- | ----------- |
| `/` | Dashboard | `stats` | `telemetry` `cache` |
| `/lists` | Lists | `list_refreshed` | — |
| `/rules` | Custom Rules | — | — |
| `/policies` | Policies | — | — |
| `/clients` | Clients | — | — |
| `/rule-tester` | Rule Tester | — | — |
| `/cache` | Cache | — | `cache` |
| `/performance` | Performance | — | — (`/history/perf` is a range query, not a poll) |
| `/upstreams` | Upstreams | — | `telemetry` `health` |
| `/settings` | Settings | `config_changed` | — |
| `/diagnostics/health` | Health | — | `health` `telemetry` |
| `/diagnostics/memory` | Memory | — | — |
| `/diagnostics/live-feed` | Live Feed | `query` | — |
| `/login` | Login | — | — |
| `/dev/gallery` | gallery (dev only) | `stats` | `telemetry` `cache` |

**The Dashboard declares no `/health`**, deliberately rather than by omission.
`Main.dc.html` draws a cluster for `/telemetry` and one for `/cache` and none for
`/health`, because the page's only `/health` figure is the uptime **tile**, and a
tile has no title bar to host a cluster. A route does not declare a polled
endpoint it cannot show a control for. The uptime tile reads the shell's one-shot
`/health` value (§7) — **p5-06 owns how that is labelled**, since a figure
fetched once at shell mount is not a live one.

**The table ships complete; the pages do not.** In p5-05 every `load` except
`/login` and `/dev/gallery` resolves to the not-yet-built empty state, and per
D7 those routes therefore acquire **nothing** — a page that renders no events
and no figures must hold no subscription. The columns above are what each route
declares once its page lands; the task that builds a row wires that row, and a
vitest case pins the mapping so it cannot drift. This is what keeps `query` on
exactly one route for the life of the phase.

Router mechanics: `pushState` + `popstate`; a `<Link>` that intercepts plain
left-clicks only (modifier keys and non-primary buttons fall through to the
browser); scroll reset on navigation; exact-match lookup with a not-found
fallback. p5-01's SPA fallback serves `index.html` for any unmatched path, so a
deep link and a refresh both land here.

---

## 10. Route-scoped lifecycle — the shell owns it

On every route change the shell runs one transition, in this order:

```text
leave:  abort in-flight requests belonging to the outgoing route
        release its event types        (subscriptions.release)
        release its endpoints          (refresh.unsubscribe)
enter:  acquire the incoming route's event types
        acquire its endpoints
        serve the retained value for each endpoint when one exists
        revalidate on that endpoint's policy
```

Release-before-acquire is deliberate: navigating between two routes that both
want `stats` must not tear the socket down and rebuild it. Because the registry
is a refcount, the union never reaches empty during that transition.

| Rule | Implementation |
| ---- | -------------- |
| **Acquire and release happen here and nowhere else** | a page component never calls `acquire`, `release`, `subscribe` or `unsubscribe`. The shell's transition is the only caller, so a page cannot leak a subscription by forgetting an unmount path. |
| **Dev-mode assertion** | after every transition, assert the socket union equals the incoming route's declared `events` and the registry's subscribed set equals its declared `endpoints`. `import.meta.env.DEV` only, so it costs the production bundle nothing. This is the net under the framework-lifecycle dependency the invariant otherwise rests on. |
| Timers | **`lifecycle/timers.ts` is the only module that may call `setInterval`, `setTimeout` or `requestAnimationFrame`.** The refresh registry and the age ticker both go through it. A unit test greps for all three outside that module — enforcement is a test, not an architectural guarantee, and saying otherwise would overstate it. |
| Listeners | every `addEventListener` in the shell has its removal in the same effect's cleanup |
| Visibility | **an input, never an actor** — see §10.1 |

### 10.1 Visibility is an input, not a second authority

The socket manager owns the connection; the refresh registry owns which
endpoints are being polled, and `lifecycle/timers.ts` owns the timers themselves.
Visibility does not open, close or reconnect anything. It sets one flag —
`suspended` — which each owner then interprets:

| Transition | What visibility does | What each owner does |
| ---------- | -------------------- | -------------------- |
| `hidden` | `suspended = true` | registry clears its timers at once; socket manager arms the `HIDDEN_CLOSE_GRACE_MS` close |
| `visible` | `suspended = false` | registry resumes and refetches anything older than that endpoint's interval; socket manager cancels a pending close, or — **only when the active route's union is non-empty** — reconnects and re-sends it. On the nine event-free screens becoming visible opens nothing. |

**Why this is a correction rather than a rewording.** With two actors able to
open and close the socket, a reconnect can race the 30 s grace timer and either
tear down a connection a route just asked for, or leave one open that nothing
wants. One owner per resource, with visibility as an input to it, makes the race
unrepresentable instead of merely unlikely.

**Ordering when both change at once** — a route change while hidden, or a
`visible` that arrives mid-transition: the route transition above runs to
completion first, then the visibility flag is applied. The transition is
synchronous and short; interleaving them is what produces the race.

---

## 11. Socket manager

One connection, cookie-authenticated, never `?token=`. `WS /api/v1/events`.

### 11.1 Subscription registry

`events/subscriptions.ts` is a `Map<EventType, number>`. `acquire(types)`
returns a `release()`. The union is the keys whose count is above zero.

| Transition | Action |
| ---------- | ------ |
| union empty → non-empty | open the socket |
| socket opens | send `{"subscribe":[…union]}` immediately |
| union changes while open | send the new union — subscribe **replaces**, and there is no unsubscribe verb |
| reconnect | re-send the current union as the first frame |
| union non-empty → empty | **close the socket**, code `1000` |

**Never `{"subscribe":[]}`.** API.md accepts an empty list and the server then
substitutes a `Ping` on the stats cadence, but the phase decided against idling
a connection at all: a closed socket frees one of the 64 slots, and re-opening
costs one handshake against nine screens that never wanted it. A vitest case
asserts an empty union produces a close and never a frame.

### 11.2 State machine

```text
              acquire()                 onopen
   closed ─────────────► connecting ───────────► open
      ▲                      │                    │
      │ release() → empty    │ close without open │ onclose / onerror
      │                      ▼                    │
      └───────────────── backoff ◄────────────────┘
                             │
                 PROBE_AFTER_FAILURES immediate failures
                             ▼
                          probing ──► authFailed ──► /login
                             │
                             └──► back to backoff (inconclusive, or
                                  session-valid → upgrade-refused detail)

  suspended (§10.1) is orthogonal to the above: it is a flag the manager reads,
  not a state in this machine. While set, `open` arms the grace close and no
  transition opens a connection.
```

| Indicator state | Condition |
| --------------- | --------- |
| `live` | `open` |
| `not needed here` | `closed` **and** the active route's union is empty |
| `reconnecting` | `connecting`, `backoff`, `probing` — anything wanted-but-not-open |

**Three states, and only three.** `aria-live="polite"` on the region across all
three, per visual-system.md §Accessibility. Only `reconnecting` is styled as a
problem, and each state carries its word as well as its colour.

`reconnecting` may carry a **detail line** — "server unreachable", or the
upgrade-refused diagnostic of §11.3.1. A detail line is secondary text inside the
existing state; it is **not** a fourth state, does not change the indicator's
colour or word, and nothing may add one. Three states is a phase-level decision
(`plan/wip/phase5/CLAUDE.md` §Two consequences, visual-system.md §Layout) and
this plan does not reopen it.

### 11.3 Backoff and the failure classification

A browser `WebSocket` exposes no HTTP status for a rejected upgrade: a `401`
arrives as `onerror` then `close(1006)`, indistinguishable from a dropped
network.

| Term | Definition |
| ---- | ---------- |
| **immediate failure** | the socket closed **without ever firing `open`**, within `PROBE_FAILURE_WINDOW_MS` of the attempt starting |
| Backoff delay | `BACKOFF_MS[min(attempt, last)]` ± `BACKOFF_JITTER` — jitter so several tabs do not resynchronize on the box |
| **Reset** | the attempt counter returns to 0 when the socket has been in `open` for **at least `BACKOFF_MS[0]` (1 s)**. Not "survives the first step" — an elapsed-time threshold against one state, so two implementations cannot disagree about it. |
| Probe trigger | `PROBE_AFTER_FAILURES` consecutive immediate failures |
| **Probe cooldown** | the counter resets to 0 when a probe fires, so the next probe needs `PROBE_AFTER_FAILURES` **fresh** immediate failures. At the 30 s backoff cap that is ≥ 90 s apart. Attempt-driven failures and the probe-driven reset are separate transitions and must be implemented as such. |

**The threshold is deliberately tolerant.** A socket failing just inside or just
outside the 2 s window is classified differently for the same server behaviour.
That costs one extra probe, or one probe delayed by a backoff step, and nothing
else — the probe is authoritative, the classification only decides *when* to ask.
Do not add hysteresis or averaging to make the boundary sharper.

The probe is **one** authenticated REST call to `GET /api/v1/policies` —
config-derived, bounded by the 16-policy ceiling, no traffic-dependent work, no
side effects, and the smallest authenticated payload in the documented surface.
**It is issued once and never retried**, and it has no backoff of its own: a
second retry loop inside the recovery path of the first is how a reconnect storm
gets built.

| Probe outcome | Classification | Action |
| ------------- | -------------- | ------ |
| `401` | session expired or invalid | clear session state, navigate to `/login` |
| `2xx` | session valid; transport or upgrade problem | keep backing off, indicator `reconnecting` |
| `5xx` | **inconclusive** — server reachable, says nothing about the session | keep backing off |
| network error / no response | **inconclusive** — server unreachable | keep backing off, `reconnecting` with an "unreachable" detail line |

**The probe answers one question: is the session still valid.** A `2xx` does not
prove the WebSocket upgrade path is healthy and is not read as if it did — it
only rules authentication out.

**Only a real authentication failure sends the user to login.**

### 11.3.1 The upgrade-refused diagnostic

After **two** probe cycles (`PROBE_DIAGNOSTIC_CYCLES`) that each returned `2xx`
while the socket has continued to fail immediately, the manager stops
reconnecting silently: the indicator **stays `reconnecting`** and gains a detail
line reading *session valid, upgrade refused*. Retrying continues at the backoff
cap. This is a detail line, **not** a fourth indicator state — see §11.2.

At `PROBE_AFTER_FAILURES = 3` and the 30 s backoff cap the detail appears roughly
**three minutes** after the first failure. That is deliberate: it is a
misconfiguration report, not an outage alarm, and firing it early would make it
a false alarm during ordinary transport loss.

This is the case a `401` from `Origin` validation produces, and it is the one
failure the probe structurally cannot classify — a valid session and a refused
upgrade look identical from the browser. It cannot occur when the dashboard is
served from its own origin, the only shipped configuration; in development it is
exactly what a missing proxy `Origin` header produces (§6), which is where the
diagnostic earns its keep.

Two cycles, not one: a single `2xx` probe during a genuine transport outage is
ordinary, and calling that a misconfiguration would be a false alarm.

### 11.4 Message dispatch

All four documented server→client types are dispatched — `query`, `stats`,
`config_changed`, `list_refreshed` — even though no shipped p5-05 route
subscribes to any of them. **Every one fans out to whoever acquired it, and none
of them touches global state.** There is no special case, which is what keeps the
event system route-scoped in fact and not only in description.

**Validation boundary.** The dispatcher checks the envelope and nothing more:
the frame parses as JSON, `type` is a string, `data` is an object. Anything that
fails drops at `debug`. An unrecognized `type` also drops — new event types may
appear. Documented fields inside `data` are **not** schema-checked at runtime;
the types describe the contract and unknown fields are ignored, per §7. Envelope
validation is a boundary against malformed input; a runtime schema would be
machinery paid for on every message to re-check a contract the server owns.

Outbound frames carry at most four event names — about 60 bytes against the
documented 4096-byte cap, which the server treats as a protocol error and closes
on. The client does **not** validate its own frame size: the maximum is fixed by
the size of the event-type set and is two orders of magnitude below the cap, so
the check would be unreachable code (§18).

### 11.5 Visibility

**The socket manager is the only thing that opens or closes the connection.**
Visibility sets `suspended` and the manager acts on it (§10.1): `suspended` arms
a `HIDDEN_CLOSE_GRACE_MS` close, clearing it cancels a pending close or, if it
already fired, reconnects and re-sends the union. Nothing outside the manager
calls `open()` or `close()`.

**A hide/show inside the grace must produce no reconnect** — a phone's normal
behaviour, and an asserted acceptance item rather than a nicety.

---

## 12. Shared bounded refresh

`refresh/registry.ts`. Three endpoints, fixed: `/health`,
`/api/v1/telemetry`, `/api/v1/cache`.

```ts
subscribe(endpoint, listener): () => void
invalidate(endpoint): Promise<void>          // manual Refresh
setRefreshInterval(endpoint, seconds): void  // the card's selector
```

### 12.1 Subscription and sharing

| Rule | Implementation |
| ---- | -------------- |
| First subscriber for an endpoint | fetch immediately, start that endpoint's timer at its current preference |
| Later subscribers | receive the last value synchronously when one exists, and cause **no** request |
| Concurrent reads | one in-flight promise per endpoint, shared by every caller. Two cards on one route reading `/telemetry` cost one request, never two loops. |
| Last unsubscribe | clear the timer and drop it. **No timer survives with zero subscribers** — the shared-not-global distinction, and the whole point of the mechanism. |
| Route scope | an endpoint is polled only while the mounted route declares it. Leaving the route releases the subscription and stops the timer, per §10. |
| **Retained value ≠ active subscription** | the last value and its timestamp survive the last unsubscribe; the **timer does not**. A retained value is served immediately on the next subscribe and then revalidated — it is a cache, never an implicit subscription, and nothing may read it as evidence that an endpoint is being polled. |
| Retained state | at most three values plus their timestamps, one per endpoint. Bounded by the endpoint set, not by uptime or pages visited. |
| Hidden document | `suspended` (§10.1) clears every timer; clearing it resumes them and refetches whatever is older than that endpoint's interval. The registry never observes `visibilitychange` itself. |
| Errors | delivered to listeners; the timer continues and **the previous value stays** — a failed refresh never blanks a card. **The registry has no `401` case at all**: `api/core.ts` owns that (§8), so session semantics live in one place and cannot drift per endpoint. |
| Nothing pushed is polled | `/stats` is never in this set. The socket pushes it. |

`use-refresh.ts` is the Preact hook wrapper: subscribe on mount, unsubscribe on
unmount. No component ever sees a timer.

### 12.2 The interval preference

Browser-local, global per endpoint, bounded (D11–D13).

| Endpoint | Options | Compiled-in default |
| -------- | ------- | ------------------- |
| `/health` | `30 s` · `1 m` · `5 m` | **`1 m`** |
| `/api/v1/telemetry` | `1 m` · `5 m` | **`5 m`** |
| `/api/v1/cache` | `1 m` · `5 m` | **`5 m`** |

Labels are what the artboards draw — `30 s`, `1 m`, `5 m`, never raw seconds.

`refresh/preferences.ts` owns one `localStorage` key per endpoint
(`fah-refresh-health`, `fah-refresh-telemetry`, `fah-refresh-cache`).

| Rule | Implementation |
| ---- | -------------- |
| Validation on read | a stored value not in that endpoint's option set falls back to the compiled default. A hand-edited `1` must never become a 1 ms timer. |
| Unavailable storage | a browser with site data blocked throws on read and on write; both are wrapped, and the compiled defaults apply |
| Change takes effect at once | `setRefreshInterval` rebuilds that endpoint's live timer immediately if one exists, and is a no-op if nothing is subscribed. **The method is deliberately not called `setInterval`** — that name is banned outside `lifecycle/timers.ts` and the grep enforcing it cannot tell a registry method from the global. |
| **Rebuild restarts from now** | elapsed time since the last fetch is discarded, not rebased. This is forced: rebasing 300 s → 60 s after 200 s elapsed would fire instantly, and a selector change must **not** issue a request. |
| Selector change issues no request | Refresh is the control for that |
| Naming | no module outside `lifecycle/timers.ts` may contain the token `setInterval`, method names included |
| Shared state, not per-control | `preferences.ts` owns a small pub/sub: every mounted selector subscribes on mount and unsubscribes on unmount, so changing one updates the others in the same render. Components never hold their own copy. |
| **Cross-tab** | `preferences.ts` listens for the `storage` event and applies a change made in another tab exactly as if it were made locally — including rebuilding a live timer. The preference is described as browser-global; without this it would be tab-global, which is a different and wronger thing. Three lines, no dependency. |

### 12.3 Manual Refresh

`invalidate(endpoint)` is what the card's Refresh button calls.

| Rule | Implementation |
| ---- | -------------- |
| Endpoint-scoped, not card-scoped | every card reading that endpoint updates from the one request |
| Coalescing | a click while a request is in flight **joins** it. Never a second request, never a queue. |
| Resets the background timer | on success the endpoint's timer restarts from now, so a manual refresh is not followed by a scheduled one seconds later |
| Failure | previous value stays visible, the envelope's `message` is shown, the timer is untouched — **identical to a background failure**, including `401`, which `api/core.ts` handles before the registry ever sees it |
| Pending state | the control is disabled while in flight and announces completion through `aria-live` |

---

## 13. Shell and component vocabulary

**Shell** — `sidebar` (four labelled sections, the nested Diagnostics group,
active item marked, ~230 px, dark in both themes per visual-system.md §Layout) ·
`topbar` (page title left; connection indicator and version right) ·
`content-header` (page name plus a short context line) · 12-column card grid.

**No restart-required banner.** It was in an earlier revision of this plan and is
**cut**: `restart_required` is carried only by the `POST /config` response and
the `config_changed` event, and p5-05 ships no route that posts config and no
route that subscribes to `config_changed`. The banner would be unreachable code
attached to a contract it cannot satisfy, and it was the single reason
`config_changed` needed global handling inside an otherwise route-scoped event
system (§11.4). It belongs to **p5-09** with Settings, where a source exists —
along with the open question of what revalidates it (§25 item 3).

**Vocabulary**, built once and used by every later task:

| Component | Contract |
| --------- | -------- |
| `tile` | large figure, label, corner glyph, accent from the fixed role table (aqua volume · red blocked · yellow ratio · green healthy · grey expected-zero), optional footer link |
| `card` | title bar, optional tool button, body; white in light, raised surface in dark |
| `table` | right-aligned `tabular-nums` figures, translucent frequency bar behind a count, zebra off, hover on, horizontal scroll inside its own container |
| `verdict-pill` | `pass` neutral · `allow` green · `block` red — **always with its word**, never colour alone |
| `status-pill` | `ok` / `degraded` / `failed` / `rejected`, same rule |
| `stage-bar` | segmented proportion bar (cache fresh/stale/expired, rule partitions) with the legend beside it |
| `empty-state` | "no data in this range" and the not-yet-built case. **An empty result is not an error.** |
| `error-state` | renders the envelope's `message` **verbatim**; `code` selects the presentation |
| `confirm-dialog` | focus-trapped, `Escape` closes, focus returns to the invoker |
| `chart` | wrapper that `await import()`s `uplot` on first render, sizes to its container, and shows a decimation footnote when told to. Proven in p5-05 with a **static** series only. |
| `refresh-cluster` | **one group bound to one endpoint**, in this order: `data-age` · interval selector · mini Refresh button. The only component that may call the refresh registry. |
| `data-age` | relative age from the registry's fetch timestamp, driven by the shared ticker below |

### 13.1 The refresh cluster — placement is drawn, not described

**The artboards in [docs/dashboard/sketch/](../../../docs/dashboard/sketch/) are
the specification.** `Cache`, `Upstreams`, `Health`, `Main` and
`MobileDashboard` were updated to carry this control; the other twelve read no
polled endpoint and are unchanged. Build against the artboard, not against this
table.

**One cluster per distinct polled endpoint on a page — never one per card.**

| Case | Placement | Drawn in |
| ---- | --------- | -------- |
| Route reads **one** polled endpoint | a single cluster right-aligned in the **content header**, beside the page title. No card carries anything. | `Cache` — five cards, all `/cache`, one cluster |
| Route reads **several** | one cluster per endpoint, anchored to the zone that owns it — a card title bar, or the status banner where `/health` owns one | `Upstreams` (`/health` on the banner, `/telemetry` on Endpoints), `Health` (`/health` under uptime, `/telemetry` on "What clients received") |
| A second card reading an endpoint that already has a cluster | **nothing** | `Health`'s Backpressure and Engine cards, `Main`'s Ruleset card — all `/telemetry`, already covered |
| Zone fed by the `stats` push, or by `/history/*` | **nothing** — there is no timer to control | `Main`'s tiles, top-N tables and the Queries-over-time chart |
| Phone | a full-width row at the top of that card's body, 44 px targets — the relocation the range selector already uses | `MobileDashboard`'s Cache card |

Where a card title bar already carries secondary text, the cluster follows it in
the same right-hand group: `entries 7,261 / 10,000` · `2 m ago` · `5 m ▾` · `⟳`.

**Interval labels are `30 s` · `1 m` · `5 m`**, matching the artboards, not raw
seconds.

**The selector is a native `<select>`** — keyboard, screen reader and the mobile
picker for free, at close to zero bytes — styled to the artboard's tokens
(`#cfd8e3` border, 4 px radius, 11.5 px `#47535f`, the same look as `.chip` and
`.btn.g`). The artboards draw it as a static span because a real `<select>`
renders an OS picker and would break their hand-drawn fidelity; the
implementation uses the real control.

**The age display needs a ticking clock, which is a timer.** One shared 30 s
ticker, refcounted, alive only while at least one `data-age` is mounted — and it
lives in **`lifecycle/timers.ts`**, not in the refresh registry. An earlier
revision put it in the registry purely to satisfy a rule worded as "the registry
owns timers", which coupled a presentation concern to the request lifecycle to
save rewording an invariant. The rule is now "**one timer module**", which both
the registry and the ticker use, and the coupling disappears.

Every other vocabulary component takes data as props and issues no requests.
`refresh-cluster` is the single exception, and confining request-triggering to
one named component is what keeps route-scoped fetching provable.

---

## 14. Theme

All colour is custom properties on `:root` in `styles/tokens.css`; the dark
theme redefines the tokens and nothing else. Explicit toggle in the top bar,
defaulting to `prefers-color-scheme`, the choice stored in `localStorage`.

**Applied before first paint** by an inline script in `index.html`, roughly:

```html
<script>try{var t=localStorage.getItem('fah-theme');
document.documentElement.dataset.theme=t||(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light')}catch(e){}</script>
```

Applied from the bundle instead, every load flashes the wrong theme. The
`try`/`catch` is load-bearing: a browser with site data blocked throws on the
read, and an uncaught throw here stops the parser before the stylesheet.
`color-scheme` is set alongside so form controls and scrollbars follow.

Colour never carries meaning alone, in either theme.

---

## 15. Responsive, accessibility, assets

| Width | Behaviour |
| ----- | --------- |
| ≥ 1200 px | full grid, sidebar expanded |
| 768–1199 px | halves become full width, sidebar collapses to icons |
| < 768 px | single column, sidebar is an overlay drawer, tables scroll inside their own container |

`body` never scrolls horizontally — asserted by comparing
`documentElement.scrollWidth` against `clientWidth` at each breakpoint. Wide
content scrolls inside its own `overflow-x: auto` container.

Accessibility at foundation level: keyboard-reachable navigation in DOM order,
visible focus rings (never `outline: none` without a replacement), `aria-live`
on the connection indicator, `aria-current` on the active sidebar item, the
drawer focus-trapped while open, colour never the only signal.

Assets: one hand-authored `assets/sprite.svg` holding only the glyphs actually
used, imported as a URL so it is emitted hashed under `/assets/` and served
`immutable`; `public/favicon.svg`. **No web font** — a system stack, per
visual-system.md §Typography. No CDN reference anywhere, and no Pi-hole string
in markup, comments, alt text, page titles or asset names — both asserted by the
postbuild greps (§4).

---

## 16. Dev-only gallery

`/dev/gallery`, registered only under `import.meta.env.DEV`. It renders every
vocabulary component in both themes with static props, and declares `stats`,
`/telemetry` and `/cache`, so a `npm run dev` session against a live `fah-api`
exercises the socket, the union re-send after a forced disconnect, the REST
probe and the refresh refcount. It is the durable surface p5-06 through p5-09
re-check their theming against.

It renders **two cards backed by the same endpoint** on purpose: that is what
proves request dedupe, selector sync and one-request-per-manual-Refresh in a
browser rather than only in vitest.

Its absence from the production bundle is asserted by the postbuild grep, not
trusted to tree-shaking.

---

## 17. Constants — `src/constants.ts`

```ts
export const REFRESH_OPTIONS_SECS = {
  health:    [30, 60, 300],
  telemetry: [60, 300],
  cache:     [60, 300],
} as const;

export const REFRESH_DEFAULT_SECS = {
  health:    60,
  telemetry: 300,
  cache:     300,
} as const;

export const REFRESH_LABELS: Record<number, string> = { 30: '30 s', 60: '1 m', 300: '5 m' };

export const AGE_TICK_MS             = 30_000;
export const HIDDEN_CLOSE_GRACE_MS   = 30_000;
export const BACKOFF_MS              = [1_000, 2_000, 4_000, 8_000, 15_000, 30_000];
export const BACKOFF_JITTER          = 0.20;
export const PROBE_AFTER_FAILURES    = 3;
export const PROBE_FAILURE_WINDOW_MS = 2_000;
export const PROBE_DIAGNOSTIC_CYCLES = 2;
export const OPEN_STABLE_MS          = 1_000;
export const BUDGET_BYTES            = 150 * 1024;
```

One module, so all of them move in one edit and none is buried in a call site.
`REFRESH_OPTIONS_SECS` is both the dropdown's contents and the validator for the
stored preference — one source, so a stored value the UI cannot offer cannot
survive a read. p5-10 may change either table from measured evidence.

---

## 18. What is deliberately *not* built

| Not built | Why |
| --------- | --- |
| Any product page | out of scope; everything after this task is a page and nothing else |
| A chart bound to real data | the wrapper is proven with a static series |
| Resource modules for endpoints the foundation does not call | §7 |
| A second fetch wrapper, anywhere | §7 |
| A boot-time session probe | the first `401` is the signal (§8) |
| CSP or other security response headers | server-side; p5-01 did not take it and this task does not add it |
| The Dashboard's own subscription set | p5-06 settles it against §23 item 3 |
| A `Pong` watchdog | p5-03 explicitly declined it server-side; the client does not compensate |
| The restart-required banner | no data source in this phase — cut and moved to p5-09 (§13) |
| A bounded page cache | speculative: no page exists to cache, so the key, the payload and the invalidation cannot be defined against anything real. p5-06 builds it when the first payload justifies it. `PAGE_CACHE_CAPACITY = 4` was a number picked, not derived. |
| **An outbound protocol abstraction** | `subscribe` is the entire client→server protocol. A layer for hypothetical future control frames is structure added for an unmeasured need — principle 16. Reviewed and **deliberately rejected**, not overlooked. |
| **Client-side frame-size validation** | the largest frame the manager can build is four event names, ~60 bytes against a 4096-byte cap fixed by the size of the event-type set. The check is unreachable by construction. Reviewed and **deliberately rejected**, not overlooked. |
| A free-text refresh interval, or an `Off` option | D13 — a typed value lets the UI degrade the resolver, and a card that never refreshes and never says so is the failure this change exists to prevent |
| A server config key for the interval | D12 — it is one browser's viewing preference, not an engine setting |
| A per-route interval override | one preference per endpoint serves every route that reads it. A page wanting it fresher right now uses Refresh. Accepted trade, recorded in §25.5 rather than discovered in p5-08. |

---

## 19. Tests — `vitest`

Pure logic, no browser, no live API. These are what make the invariant cheap to
hold across the next four tasks.

| Area | Cases |
| ---- | ----- |
| Subscription registry | union is the positive-count keys · acquire/release refcounts · two routes wanting `stats` do not close the socket between them · empty union closes and never sends `{"subscribe":[]}` · reconnect re-sends the current union |
| Backoff | schedule and cap · jitter stays inside ±20 % · **reset only after `open` has held for `OPEN_STABLE_MS`** · an open that dies sooner does not reset |
| Probe classification | `401` → login · `2xx` → keep backing off · `5xx` → **inconclusive**, keep backing off · network error → **inconclusive**, unreachable detail · **the probe is issued once and never retried** · counter resets after a probe, so the next needs `PROBE_AFTER_FAILURES` fresh failures |
| Immediate-failure detector | close-without-open inside the window counts; a close after a healthy open does not |
| Shared refresh | first subscriber fetches · a second causes no request · concurrent callers share one in-flight promise · **last unsubscribe clears the timer** · setting `suspended` clears every timer and clearing it refetches whatever is stale · each endpoint runs at its own interval, not one shared period · a failed refresh keeps the previous value |
| Interval preference | a stored value outside the option set falls back to the compiled default · a throwing `localStorage` falls back and does not crash · `setRefreshInterval` rebuilds a live timer **from now**, discarding elapsed time · it issues **no** request · with nothing subscribed it is a no-op · every mounted selector for one endpoint reads one value · a `storage` event from another tab applies identically |
| Manual Refresh | endpoint-scoped, so two cards update from one request · a click during an in-flight request joins it and starts nothing · success restarts the background timer · failure keeps the previous value and surfaces the envelope's `message` |
| Age ticker | one shared 30 s ticker · refcounted, alive only while a `data-age` is mounted · last unmount clears it |
| Cluster placement | a route with one polled endpoint renders exactly one cluster and no card carries one · a route with several renders exactly one per endpoint · no route renders two clusters for the same endpoint · a route with no polled endpoint renders none |
| Retained value | survives the last unsubscribe · the timer does not · a later subscribe is served the retained value **and** triggers a revalidation · at most three retained values |
| Timer module | `lifecycle/timers.ts` is the only module containing the tokens `setInterval`, `setTimeout` or `requestAnimationFrame` — method names included, which is why the registry's selector API is `setRefreshInterval` |
| Connection ownership | `suspended` alone drives the hidden/visible path · nothing outside the socket manager opens or closes · a route change while suspended does not open a connection |
| Envelope validation | non-JSON, missing `type`, non-object `data` and unknown `type` all drop without throwing · a valid envelope with unexpected fields inside `data` still dispatches |
| Upgrade-refused diagnostic | two `2xx` probe cycles with continued immediate failures attach the detail line · one does not · **the indicator stays `reconnecting`** and no fourth state exists |
| Visibility | hide/show inside the grace produces no close and no reconnect |
| Route table | exactly one route declares `query` · every declared type is one of the four documented names · no route outside the mapping declares events |
| API core | envelope parsed · unknown `code` preserved · unknown fields ignored · `Retry-After` parsed · `503` without `Retry-After` marked non-retryable · `401` reaches the guard once |
| Size gate | the comparison fails above `BUDGET_BYTES` and passes at it |

---

## 20. Verification procedure — the measured acceptance

Read off a request log against a **running** `fah-api` producing real traffic,
never off the source. Every result is recorded in the review file with the
build, the browser and the API version it was taken against.

| # | What | How |
| - | ---- | --- |
| V1 | Bundle serves unchanged through p5-01's handler | build the image, run it, load `/`: `text/html` on the shell, `immutable` on `/assets/*`, `no-cache` on the shell, `content-encoding: br` when offered, and a deep link (`/settings`) resolving through the SPA fallback |
| V2 | MIME coverage | one request per emitted extension — `.js` `.css` `.svg` `.json` and the icon |
| V3 | Sign-in end to end | log in with the generated password: `204` + `Set-Cookie`, shell loads. Delete the cookie; the next authenticated call returns to `/login` rather than showing an error. |
| V4 | Socket lifecycle | on the gallery route: connects, sends the union, forced close, reconnects with backoff, **re-sends the union**. Indicator observed in all three states. |
| V5 | No `query` in the shell | with the API producing DNS traffic, sit on a route whose union is `stats` for 60 s: zero `query` frames received, and `has_query_subscribers()` never true server-side |
| V6 | Probe classification | (a) expire the session with `logout-all` from another client → socket fails → probe → `/login`. (b) stop the container → socket fails → probe → **stays** reconnecting, no navigation to login. |
| V7 | Closed socket is the steady state | mount a route declaring no events; confirm **server-side by connection count** that no socket is open — not merely that the browser shows nothing |
| V8 | Subscription add/remove | entering a route that needs a type opens or extends the union; leaving removes it; removing the last closes the connection |
| V9 | Refresh discipline | two cards on one endpoint → **one** request per that endpoint's interval; zero subscribers → **zero requests**; navigating away stops the outgoing route's requests within one interval; each endpoint observed at its own period, not a shared one |
| V9a | Interval selector | placement matches the artboard for each of the five updated screens — count the clusters and compare. Then change `/telemetry` from `5 m` to `1 m`: **no request is issued by the change**, the next scheduled request arrives 60 s later (not sooner — elapsed time is discarded), navigating to another route that reads `/telemetry` shows the new value, and the choice survives a reload |
| V9b | Manual Refresh | one click updates every card on that endpoint from one request; three rapid clicks issue **one** request; the background timer restarts from the manual fetch; a refresh against a stopped API leaves the previous figures on screen and shows the envelope's message |
| V9c | Data age | every endpoint-backed card shows an age that advances without a reload; a 5-minute-old `/telemetry` card and a 2-second-old `stats` tile are distinguishable on the same screen |
| V10 | Backgrounding | hide → polling stops immediately, socket closes after 30 s; show → subscriptions restored exactly. **Hide/show inside 30 s → no reconnect**, asserted from the server's connection log. |
| V11 | Ten-minute idle | sit on one route for 10 min: the request log holds nothing that route does not need. Heap snapshot before and after visiting all 13 screens — no growth with pages visited. |
| V11a | Single connection authority | hide the tab and navigate at the same moment, repeatedly: no connection is opened while suspended, none is left open with an empty union, and the grace timer never races a reconnect. Read from the server's connection count, not the browser. |
| V11b | Cross-tab preference | change `/telemetry` to `1 m` in one tab; a second tab showing the same endpoint updates its selector and rebuilds its timer without a reload |
| V12 | Initial cost | initial request count and JS parse cost recorded; **login proven not to fetch the chart chunk**, from the network panel |
| V13 | Theme and layout | both themes at 1400 / 900 / 400 px; no horizontal body scroll; **no theme flash** on a hard reload with dark stored and a light system preference |
| V14 | Size gate | prove it fails once (§4); record the final gzip **and** brotli totals against 150 KB |
| V15 | Image | rebuilt image size against the 30 MB budget; no Node and no `node_modules` in the runtime layer |
| V16 | Cargo gates | `fmt --check`, `clippy --workspace --all-targets -- -D warnings`, `test --all-features --workspace` — the workspace is untouched, but they still run |

---

## 21. Implementation units

Each unit ends compiling, typechecking, and with its own tests green.

| U | Unit | Files | Done when |
| - | ---- | ----- | --------- |
| U1 | Project skeleton | `package.json`, `package-lock.json`, `tsconfig.json`, `vite.config.ts`, `index.html`, `.gitignore` | `npm run typecheck` and `npm run build` pass on an empty app |
| U2 | Postbuild: compression, size gate, forbidden-content greps | `scripts/postbuild.mjs` + tests | siblings emitted, table printed, gate proven to fail over budget |
| U3 | Dockerfile frontend stage | `Dockerfile` | image builds, `/web` holds the real bundle, `COPY --from=frontend /web /web` untouched, `cargo test -p fah-api` still green |
| U4 | Design tokens, base CSS, theme | `styles/*`, `theme/theme.ts`, pre-paint script | both themes render, no flash on reload |
| U5 | API core and the four modules | `api/*` | envelope, `ApiError`, `Retry-After`, `401` hook, all unit-tested |
| U6 | Router, route table, `<Link>` | `router/*` | navigation, deep links, `popstate`, mapping test green |
| U7 | Subscription registry, backoff, probe | `events/subscriptions.ts` `backoff.ts` `probe.ts` | the §19 suite green |
| U8 | Socket manager | `events/socket.ts` | state machine, union re-send, close-on-empty, dispatch of all four types |
| U8a | Timer ownership | `lifecycle/timers.ts` | sole caller of `setInterval` / `setTimeout` / `requestAnimationFrame`; the grep test covers all three |
| U9 | Shared bounded refresh | `refresh/registry.ts`, `refresh/use-refresh.ts` | refcount, shared in-flight, last-unsubscribe-stops, per-endpoint intervals, `invalidate()`, retained value ≠ active subscription, no `401` case |
| U9a | Interval preference store | `refresh/preferences.ts` | option sets are the validator, throwing storage falls back, `setRefreshInterval` rebuilds from now and issues no request, pub/sub, `storage` event |
| U10 | Visibility signal and the route transition | `lifecycle/visibility.ts`, shell transition | `visibility.ts` observes `visibilitychange` and sets `suspended` — **it owns no timer and never opens or closes anything**; the grace close belongs to the socket manager (U8). Route transition is the only caller of acquire/release, with the DEV union-vs-declaration assertion. No reconnect on a fast hide/show. |
| U11 | Shell chrome | `shell/*` | sidebar with 13 entries and the nested group, top bar, indicator in three states, content header, card grid |
| U12 | Component vocabulary | `components/*` | all ten, both themes |
| U12a | `refresh-cluster` and `data-age` | `components/refresh-cluster.tsx`, `components/data-age.tsx`, the shared ticker in `lifecycle/timers.ts` | matches the artboards: one cluster per polled endpoint per page, content-header placement on a single-endpoint route, zone placement otherwise, 44 px body row on the phone |
| U13 | Login page and guard | `session/*`, `pages/login.tsx` | every documented failure rendered from the envelope |
| U14 | Dev gallery + not-yet-built page | `pages/*` | gallery renders everything; absent from the production bundle, asserted |
| U15 | Verification pass | — | V1–V16 **plus V9a, V9b, V9c, V11a, V11b** recorded in the review file |
| U16 | Documentation | root `CLAUDE.md`, `information-architecture.md`, `visual-system.md`, `p5-10-phase5-verification.md` | **proposed in §23, applied only after approval** |

---

## 22. Acceptance criteria mapping

| Task criterion | Covered by |
| -------------- | ---------- |
| Static bundle served unchanged by p5-01's handler, `.br` / `.gz`, MIME | U2, U3, V1, V2 |
| Typecheck passes, no implicit any | U1, gate |
| Sign-in end to end; expired session returns to login | U13, V3 |
| Socket connects, subscribes, survives a disconnect, re-sends; three states | U8, U11, V4 |
| Shell receives no `query` | U7, U8, V5 |
| Rejected upgrade classified by the probe | U7, V6 |
| One request per endpoint per interval; none with no subscriber | U9, V9 |
| One cluster per polled endpoint per page, placed as the artboards draw it | U12a, V9a |
| Interval global per endpoint, no request on change | U9a, U12a, V9a |
| Manual Refresh coalesces, resets the timer, keeps data on failure | U9, U12a, V9b |
| Every polled endpoint on a page shows its data age | U12a, V9c |
| Leaving a page stops its requests within one interval | U10, V9 |
| Route needing no events → socket closed, server-side count | U8, V7 |
| Enter adds / leave removes / last removal closes | U7, U8, V8 |
| Backgrounding; no reconnect on a fast hide/show | U10, V10 |
| One connection authority — no open while suspended, no race with the grace timer | U8, U10, V11a |
| The interval preference is browser-global, including across tabs | U9a, V11b |
| Ten minutes idle; memory flat across 13 pages | U9, U10, V11 |
| Initial request count, parse cost, login without the chart chunk | U1, V12 |
| Both themes, three breakpoints, no horizontal scroll, no flash | U4, U11, U12, V13 |
| Size gate fails when exceeded — proven once | U2, V14 |
| Final size recorded, gzip **and** brotli | U2, V14 |
| Cargo gates green | V16 |

---

## 23. Documentation edits proposed — awaiting approval

Root CLAUDE.md §Working agreement 1 applies. Listing is not permission; nothing
below is applied until the owner says yes.

**1. Root `CLAUDE.md` §Layout.** Replace

> `dashboard/     # empty until the dashboard phase — do not scaffold`

with

> `dashboard/     # frontend/ — Vite + TypeScript + Preact dashboard (p5-05)`

**2. `docs/dashboard/information-architecture.md` §Cross-cutting behaviour**,
the paragraph beginning "The socket subscribes; it does not simply listen."
It says the baseline is `stats`, `config_changed` and `list_refreshed` with the
Live Feed adding `query`. That contradicts the four paragraphs below it in the
same file, the phase `CLAUDE.md` invariant, and this task. Proposed replacement:

> **The socket subscribes; it does not simply listen.** There is no shell
> baseline: each route declares the event types it renders, and the union of the
> mounted route's declarations is the subscription. It is re-sent after every
> reconnect, and when the union is empty the connection is closed rather than
> idled. Server-side filtering is what makes this worth doing — it removes the
> bandwidth *and* the lag, since a stats-only socket sends one message every two
> seconds and cannot fall behind on query volume the way an unfiltered one does.

**3. `docs/dashboard/information-architecture.md` §Dashboard**, the sentence
"The socket on this page subscribes to `stats`, `config_changed` and
`list_refreshed`." Under the route-scoped mapping the Dashboard declares
`stats`; `list_refreshed` belongs to Lists and `config_changed` to Settings.
**Recommendation: leave this one to p5-06**, which builds the Dashboard and is
where the consequence is real. Recorded here so it is not discovered mid-task.

**4. `docs/dashboard/visual-system.md` — APPLIED.** §Layout gains the refresh
cluster and its placement rule, and the file's opening states that the built
dashboard matches the artboards exactly. An earlier version of this edit
described a control family the artboards did not have; it was corrected against
the drawing.

**4a. `docs/dashboard/sketch/` — APPLIED.** `Cache.dc.html`,
`Upstreams.dc.html`, `Health.dc.html`, `Main.dc.html` and
`MobileDashboard.dc.html` now draw the cluster. Twelve artboards read no polled
endpoint and are unchanged. The sketch README's rule — a drifted sketch is worse
than none — is why the drawing moved before the code.

**5. `plan/wip/phase5/p5-10-phase5-verification.md`** gains the endpoint-cost
measurement the interval defaults are currently guesses against. Proposed
addition to its scope:

> - **Cost of the polled endpoints on the RB5009** — `GET /health`,
>   `GET /api/v1/telemetry`, `GET /api/v1/cache`: median service time and CPU
>   cost per call, with `/cache` measured at two cache occupancies to establish
>   whether stage counting is proportional to entries. The p5-05 interval
>   defaults and option sets (60 s / 300 s, and the absence of a 30 s option on
>   `/telemetry` and `/cache`) are provisional and are corrected here from the
>   measured figures, in `dashboard/frontend/src/constants.ts`.

**6. `plan/wip/phase5/p5-10-phase5-verification.md` — a second proposal, and a
proposal only.** Screenshot diffing of the built UI against the artboards, so
fidelity stops depending on someone remembering to compare by eye across four
page tasks.

**p5-05 introduces no Playwright and no diffing.** It records the proposal and
nothing else; p5-10 decides whether it enters scope. If it does, the baseline is
worthless unless it is pinned:

> - fixed viewport per breakpoint, fixed browser build, `prefers-color-scheme`
>   and the stored theme both set explicitly, and every date, uptime and counter
>   frozen — the artboards carry fixed figures, the app does not;
> - **the font is the blocker, not a detail.** Every artboard loads IBM Plex from
>   `fonts.googleapis.com`, while the product ships a system stack and no web
>   font (visual-system.md §Typography). Text metrics therefore differ by
>   construction and a pixel diff can never pass. Any diffing here compares
>   **structure** — box geometry, element presence, ordering — not pixels, or it
>   compares the app against a stored app baseline and uses the artboards only
>   for human review.

Without both, this produces noisy diffs and false positives, which is worse than
no check because it trains people to ignore it.

---

## 24. Risks and traps

| Risk | Mitigation |
| ---- | ---------- |
| **Module preload undoing the split.** Vite's default injects preload links that eagerly fetch the lazily-split chunks. | `build.modulePreload: false`; V12 proves login does not fetch the chart chunk. |
| **The dev `Origin` trap** (§6). A missing proxy header makes every dev WebSocket a `401`, which the probe reads as an expired session — an infinite bounce to `/login` that looks like an auth bug. | The header is in `vite.config.ts` from U1, and the trap is carried into the review file. |
| **`Secure` cookie in development.** Safari refuses it on `http://localhost`. | Documented; Safari goes through the built image. |
| **Vite output drifting from p5-01's handler.** `/assets/*` is `immutable`, everything else `no-cache` with an SPA fallback — so a *missing* file under `assets/` is a `404`, while a missing one elsewhere silently returns the shell with `200`. | `assetsDir: 'assets'`, `assetsInlineLimit: 0`, and V2 walks every emitted extension. |
| **Budget blown by one convenience dependency.** 150 KB is generous for this design and trivial to lose. | The gate is inside `npm run build`, so it fails the image build too, not only a local check. |
| **The gallery reaching production.** | Postbuild grep, asserted, not trusted to tree-shaking. |
| **A timer surviving its page** — the most likely way this invariant rots in p5-06 onward. | `lifecycle/timers.ts` is the only timer owner; the shell's route transition is the only caller of acquire/release; a dev-mode assertion checks the union against the route declaration after every transition; V9 and V11 measure it. |
| **`{"subscribe":[]}` as an idle state.** The server accepts it, and it silently defeats the peer-death watchdog. | Unit-tested: an empty union closes and never sends a frame. |
| **`503` retry storm.** `api.tls = false` returns `503` with no `Retry-After`, for ever. | A `Retry-After`-less `503` is marked non-retryable and the UI says why. |
| **Node reaching the runtime image.** | Multi-stage; the runtime layer copies emitted files only; V15 confirms. |
| **First-run password.** It is printed once to the container log and returned by no route. | The task's verification needs it; capture it from the log before starting, or delete `/config/auth-hash` and restart to regenerate. |
| **Five-minute-old figures reading as current.** At a 300 s interval an unlabelled figure is a lie, and on the Dashboard it sits beside 2 s push tiles. | `data-age` is the cluster's first element, so a polled endpoint cannot ship a Refresh without its age. V9c asserts the two ages are distinguishable on one screen. |
| **A stray timer outside the one timer module.** | `lifecycle/timers.ts` is the sole caller of `setInterval`, `setTimeout` and `requestAnimationFrame`; a test greps for all three elsewhere. **This is test enforcement, not an architectural guarantee** — JS has no way to prevent an equivalent scheduler without a lint rule, and claiming otherwise would oversell it. |
| **Selector desync.** Two clusters for one endpoint on different routes, each holding its own copy of the preference, drift apart. | The control renders from shared preference state, never from local state; V9a asserts the value carries across routes. |
| **Drifting from the artboards** — the failure this task is most likely to cause, because it builds the vocabulary twelve later screens inherit. | The artboard wins over prose, always. Every screen is compared against its `.dc.html` before its task is called done, and a deliberate deviation edits the artboard in the same change. |
| **Inventing chrome the sketch does not have.** Already happened once in this plan: the refresh cluster was first specified as a control family that exists nowhere in the drawing. | Read the artboard before writing a component spec. Where it is silent, ask rather than invent. |

---

## 25. Contradictions recorded, not silently resolved

1. **information-architecture.md's baseline sentence** — §23 item 2, proposed.
2. **information-architecture.md §Dashboard's three-type subscription** — §23
   item 3, deferred to p5-06.
3. **The artboards are drawn in a font the product will not ship.** Every
   `.dc.html` loads IBM Plex from `fonts.googleapis.com`; visual-system.md
   §Typography ships a system stack and no web font. The artboards therefore
   specify layout, density and placement — not glyph rendering — and "looks
   exactly like the sketch" means structure, not text metrics. This is the
   binding constraint's one genuine hole and it is what makes pixel diffing
   impossible (§23 item 6). Recorded, not resolved: changing the artboards to a
   system stack would make them render differently on every machine that opens
   them, which is its own loss.
4. **The restart-required banner cannot revalidate from `/health`.** The phase
   `CLAUDE.md` says it revalidates "opportunistically whenever a mounted page's
   shared refresh reads `/health`", but `GET /health` carries only `status`,
   `version` and `uptime_seconds` — `restart_required` appears solely in the
   `POST /config` response and the `config_changed` event (API.md lines 843,
   846, 897, 1148). **p5-05 no longer builds the banner** (§13), so nothing here
   ships against a contract it cannot satisfy. **p5-09 owns the resolution**:
   either a documented field it can read, or the phase `CLAUDE.md` sentence
   corrected.
5. **One preference per endpoint serves every route that reads it** — an
   accepted trade, decided rather than discovered. Performance cannot want
   `/telemetry` fresher than the Dashboard does. A page that wants it now uses
   Refresh. Recorded here so p5-08 does not reopen it as a bug.
6. **The phase table lists p5-04 as `WAITING`** while `c8603dd` implements it
   and its review file is complete. The table is stale, not the work. Moving a
   task's status is the owner's call, so it is reported here and not changed.
