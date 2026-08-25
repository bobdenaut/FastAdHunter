# P5-05 — Frontend Foundation

**Phase:** 5 · **Depends on:** p5-04 · **Model:** Opus

## Goal

The application shell exists and works: it signs in, holds a live socket
subscribed to what it actually needs, renders the navigation and the card
vocabulary in both themes at every breakpoint, and fails the build if it grows
past its budget. No product page yet — everything after this task is a page and
nothing else.

## Context

Root CLAUDE.md's layout note says `dashboard/` is empty until the dashboard phase
and must not be scaffolded. This task is that phase: the note is updated in the
same change.

Stack is fixed by
[docs/dashboard/visual-system.md](../../../docs/dashboard/visual-system.md):
TypeScript + Preact + Vite, uPlot for time series, hand-drawn SVG for donuts, own
CSS. No AdminLTE, Bootstrap, jQuery or DataTables. No CDN — the box may be the
network's only resolver, and a UI that needs the internet fails exactly when it
is needed.

The API contracts are frozen: `p5-03` settled the `/events` subscription protocol
and the `GET /clients` policy fields, `p5-04` settled the session. The typed
client is written against those, not against guesses.

## Scope

- **Project** at `dashboard/frontend/`: Vite + TypeScript + Preact, static build,
  output consumed by the `frontend` Docker stage from `p5-01` — which replaces
  its fixture bundle with this build in the same change. Strict TypeScript.
- **Typed API client**, hand-written from API.md — one module per resource group,
  one error type from the documented envelope, unknown fields ignored rather than
  rejected. Hand-written on purpose: there is no OpenAPI document, and writing
  the types is how the contract gets read carefully.
- **Socket manager** for the events WebSocket: one shared connection, reconnect
  with backoff, dispatch of the four message types, exposed connection state.
  Authenticates by cookie; never by query-string token.
  - **Subscriptions are route-scoped and the connection is not persistent.**
    Pages register the event types they render on mount and release them on
    unmount; the manager sends one `subscribe` message for the union and re-sends
    it after every reconnect. **There is no shell baseline subscription** — if no
    mounted page needs events, the manager closes the connection rather than
    holding it idle. Opening it is one page's decision, not the shell's.
  - Which route needs what: `stats` → Dashboard. `list_refreshed` → Lists.
    `config_changed` → Settings. `query` → Live Feed, and nowhere else. The other
    nine screens need none, and on them the socket is legitimately closed.
  - **Authentication failure must not become an infinite reconnect loop.** A
    browser `WebSocket` exposes no status for a rejected upgrade, so a `401`
    arrives as an indistinguishable `onerror`. After repeated immediate failures
    the manager makes one authenticated REST probe and classifies the outcome:
    expired or invalid session → return to login; transient transport failure →
    keep backing off; server unreachable → show it as such. Only a real
    authentication failure sends the user to login.
  - Disconnects are otherwise expected — slow consumers are dropped by design —
    so the indicator is informational, not an error.
- **Login page** against `p5-04`, and a route guard that returns to it on `401`.
- **Shell**: sidebar with the four sections and the nested Diagnostics group, top
  bar with connection state and version, content header, card grid.
- **Component vocabulary**, built once and used by every later task: tile, card,
  table with tabular figures and frequency bars, verdict pill, status pill, stage
  bar, empty state, error state, confirm dialog, chart wrapper.
- **Shared bounded refresh**, built here and used by every page after it. One
  mechanism reads `/telemetry`, `/cache` and `/health` on a slow interval, with a
  single in-flight request shared by every widget that wants the same response.
  No page starts its own timer, and nothing polls what the socket already pushes.

  **Shared, not global.** It polls an endpoint only while at least one mounted
  page subscribes to it; the last unsubscribe clears that timer. A refresher that
  keeps reading `/telemetry` because it exists is the exact thing the route-scoped
  invariant forbids.

- **Route-scoped lifecycle**, per the invariant in the phase `CLAUDE.md`. The
  shell owns it so no page has to remember:
  - unmounting a route clears its timers and releases its event types;
  - re-entering may serve cached data immediately, then revalidates on that
    page's policy; the page cache is bounded and does not grow with the number of
    screens visited;
  - on `hidden`, rendering and polling stop at once and the socket closes after a
    short grace period — not on the event edge, or a phone pays a TLS handshake
    per app switch;
  - on `visible`, the connection is recreated and the active route's
    subscriptions restored.
  - **Never idle a socket with `{"subscribe":[]}`.** The 2 s stats cadence is what
    lets the server detect a peer that vanished without closing; a silent socket
    holds a connection slot until TCP gives up. Close it instead.
- **Connection indicator — three states.** **live** · **not needed here** ·
  **reconnecting**. A closed socket is the correct steady state on nine of the
  thirteen screens, so a two-state indicator would report a fault on pages that
  never asked for a connection. `aria-live` applies to all three.
- **Router**: name it. A ~1 KB library or a small `popstate` handler — either is
  fine, but `p5-01`'s SPA fallback implies history-API routing, so the choice is
  made here rather than assumed.
- **Theme**: light and dark as tokens on the root, explicit toggle defaulting to
  the system preference. Colour is never the only signal. **The stored choice is
  applied before first paint** by a tiny inline script in `index.html`; without
  it every load flashes the wrong theme.
- **Responsive**: the three breakpoints in visual-system.md. The body never
  scrolls horizontally; wide content scrolls inside its own container.
- **Self-hosted assets**: a small SVG sprite. **No bundled web font** — a subset
  WOFF2 is 15–40 KB already-compressed, brotli takes nothing further off it, and
  it adds a request on the critical path. Use a system font stack unless a named
  visual requirement justifies the cost, and if one is claimed, state it.
- **Bundle shape**: **3–4 meaningful chunks**, not one and not dozens. Roughly:
  shell and login; charts; infrequently-visited pages. Login must not pull uPlot.
  Configure Vite's module-preload behaviour explicitly — the default injects
  preload links that can eagerly fetch the very chunks that were split out, which
  spends the split and buys nothing. Keep `build.sourcemap` false, set an explicit
  modern `build.target`, and do not add `@vitejs/plugin-legacy`.
- **Pre-compressed output**: this task owns adding the `.br` / `.gz` generation
  the handler from `p5-01` selects between. Vite does not emit them on its own.
- **Size gate**: the build fails over 150 KB gzip, wired into the build script.
  **Brotli is measured and reported alongside** — gzip gates because it is the
  stricter bound, brotli because it is what actually travels.
- **Doc update in the same change**: root CLAUDE.md's layout line, since
  `dashboard/` is no longer empty. Repository contract document — propose the
  edit and wait for approval.

## Acceptance criteria

- The build produces a static bundle that `p5-01`'s handler serves unchanged,
  including `.br` / `.gz` siblings and correct MIME types.
- Typecheck passes with no implicit any.
- Signing in works end to end against a running `fah-api`; an expired session
  returns to the login page rather than showing an error.
- The socket connects, sends its subscription, survives a forced disconnect, and
  re-sends the subscription after reconnecting. Connection state is visible, in
  all three states.
- **The shell does not receive `query` events** — asserted against a running API
  producing traffic, not by inspection.
- A rejected upgrade is classified by the REST probe: an expired session returns
  to login, an unreachable server does not.
- The shared refresh mechanism issues one request per endpoint per interval no
  matter how many widgets read it, and **none at all when no mounted page
  subscribes to that endpoint**.

### Route-scoped fetching — the acceptance bar

Measured with a request log against a running API, not read off the source. The
objective is **approximately zero API activity attributable to an inactive
page**, and these are what prove it:

- Navigating away from a page stops every request that page was making, within
  one refresh interval. Asserted per page as each one lands.
- **With a route mounted that needs no events, the WebSocket is closed** — not
  open and idle. Verified on the server side by connection count, not only in the
  browser.
- Entering a page that needs an event type opens or extends the subscription;
  leaving it removes that type, and removing the last one closes the connection.
- Backgrounding the tab stops route polling immediately and closes the socket
  after the grace period; foregrounding restores exactly the active route's
  subscriptions. A rapid hide/show inside the grace period causes **no**
  reconnect — asserted, because this is the phone's normal behaviour.
- Sitting on any page for ten minutes issues no request that page does not
  need, and the client's memory does not grow with the number of pages visited.
- Initial request count and initial JS parse cost recorded; login proven not to
  load the chart chunk.
- Shell renders correctly in both themes at all three breakpoints, with no
  horizontal body scroll and no theme flash on load.
- The size gate fails the build when deliberately exceeded — prove it once.
- Final bundle size recorded, **gzip and brotli**, against the 150 KB budget.
- Cargo gates green: the workspace is untouched, but they still run.

## Out of scope

Every product page. Any chart bound to real data — the chart wrapper is proven
with a static series.

## Suggested prompt

> Read docs/dashboard/visual-system.md,
> docs/dashboard/information-architecture.md, API.md, the p5-03 and p5-04 review
> files, and plan/open/phase5/p5-05-frontend-foundation.md. Scaffold
> dashboard/frontend/ with Vite, TypeScript and Preact, write the typed API
> client and the socket manager with its subscription and REST-probe failure
> classification, build the shell, the shared bounded refresh and the component
> vocabulary in both themes, wire the login flow, split the bundle into 3–4
> chunks with explicit module-preload configuration, add `.br`/`.gz` generation,
> and add the bundle-size gate reporting gzip and brotli. Propose the root
> CLAUDE.md layout-note edit and wait for approval.
