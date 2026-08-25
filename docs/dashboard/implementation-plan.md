# Implementation Plan

Derived from the finalized design decisions. The source of truth for *what* is
built is [capability-matrix.md](capability-matrix.md) and
[information-architecture.md](information-architecture.md); for *how it looks*,
[visual-system.md](visual-system.md). This file is the order of work.

**The executable form of this plan is `plan/open/phase5/`.** Where the two
disagree, the task files win — they carry the acceptance criteria. This file
explains the shape; the tasks are what gets worked.

Four things gate the frontend, and all four are settled in code before a page is
written: static serving, the certificate a browser must accept, the API contracts
the typed client is written against, and the session. Nothing is built on an
assumption that later forces a rewrite.

## Frozen decisions

- TypeScript + Preact + Vite. Static bundle only. No SSR, no Node runtime in
  the appliance.
- uPlot for time series, hand-drawn SVG for donuts.
- Own CSS. No AdminLTE, Bootstrap, jQuery, DataTables. **No web font** — system
  stack.
- Under 150 KB gzip, whole bundle; brotli reported alongside. No CDN.
- **3–4 chunks**, route-level lazy loading, module-preload configured
  explicitly. Login does not load the chart chunk.
- Pi-hole is the visual and interaction reference, never the domain model.
- One origin: `fah-api` serves the bundle from `/web`, baked into the runtime
  image by a multi-stage build. The UI and the API ship and version as one
  artifact; `/web` is never a volume.
- Session cookie auth per
  [auth-design-draft.md](auth-design-draft.md). No API key in
  `localStorage` as the normal mechanism.
- The event socket **subscribes**. Default is every event for backward
  compatibility; the dashboard narrows to what each page needs, and only the Live
  Feed asks for `query`.
- **Route-scoped data fetching.** An inactive page has approximately zero API
  activity attributable to it. Timers and subscriptions belong to the mounted
  route; the shared refresh polls only endpoints a mounted page wants; the socket
  is closed when no active route needs events, and closed after a grace period
  while the document is hidden. Never idled by subscribing to nothing — that
  removes the traffic the server's dead-peer watchdog needs.
- Connection indicator: **live** · **not needed here** · **reconnecting**. Only
  the last is a fault.

## One correction to the routing sketch

The finalized routing lists `/events` at the root. The WebSocket is
**`/api/v1/events`** — documented in [API.md](../../API.md) and mounted
under the `/api/v1` nest in `crates/fah-api/src/routes.rs`. Moving it to the
root would break the published contract for every existing client for no gain,
since the cookie is same-origin either way.

Routing as it should stand:

| Path | Serves |
| --- | --- |
| `/` | `index.html` (SPA entry) |
| `/assets/*` | content-hashed static assets |
| `/api/v1/*` | the API, including `WS /api/v1/events` |
| `/health` | unchanged, unauthenticated |

## Stages and tasks

The stage letters below are this file's own; the executable unit is the task.

| Stage | Task | What |
| ----- | ---- | ---- |
| 0 | — | what the code does today |
| A | `p5-01` | serve the bundle from `fah-api` |
| A′ | `p5-02` | the certificate a browser will accept |
| A″ | `p5-03` | freeze the API contracts |
| B | `p5-04` | authentication |
| C | `p5-05` | frontend foundation |
| D | `p5-06` … `p5-09` | pages |
| E | `p5-10` | packaging and verification |

## Stage 0 — what the code actually does today

Verified 2026-08-25 against `crates/fah-api/`, and each line is a thing the plan
below had to be corrected against.

| Verified | Consequence |
| -------- | ----------- |
| `fah-api` serves no static files. `router()` is `/health` + `.nest("/api/v1", …)` + `.fallback(not_found)`, wrapped in `require_api_key`. No `ServeDir`, no `ServeFile`, no `tower-http` in the crate. | Nothing to reuse; Stage A builds it. |
| Auth is bearer-only. `require_api_key` layers the whole router with `/health` exempt, by exact path match on a one-element list. The WebSocket takes `Authorization` or `?token=`. | Stage B builds the session. The static exemption is a **positive allowlist**, never "anything outside `/api/v1`". |
| `.dockerignore` excludes `/dashboard` outright. | The frontend build stage cannot see its own source until that is fixed. Stage A owns it. |
| `crates/fah-api/tests/request_coverage.rs` scrapes `router()` and demands a fixture per route. | Every task that adds a route adds a fixture or an `UNCOVERED` entry, or the gates go red. |
| `GET /config` returns the whole `fah_config::Config`; its comment asserts nothing in it is secret. | Adding `[auth]` breaks that. Stage B redacts `auth.*` and rejects it on `POST`. |
| One multi-thread Tokio runtime serves DNS, HTTP and API. | Argon2id runs on `spawn_blocking`, behind a concurrency bound. |
| The generated certificate's SANs are `fastadhunter`, `localhost`, `127.0.0.1`. | Nothing covers the LAN address the dashboard is opened at. Settled before the session is built. |
| `WS /events` sends every event to every socket and ignores client messages. | The subscription protocol is added before the typed client exists. |

**Done when:** static serving works, a browser is known to accept this box's
certificate and keep a `Secure` cookie, the `/events` and `/clients` contracts
are frozen, and the session exists — so the frontend is built against a stable
contract rather than a hope.

## Stage A — serve the bundle from `fah-api`

Smallest change that gives one origin.

**A.1 Where the assets live — decided.** A multi-stage Docker build: a
`frontend` stage runs the Vite build, and the runtime stage takes the output
with `COPY --from=frontend … /web`. The Node toolchain and `node_modules` stay
in the build stage and never reach the shipped image. `fah-api` serves `/web`.

```text
dashboard/frontend/  --npm run build-->  dist/
                                          |
                              COPY --from=frontend
                                          v
                     runtime stage (distroless, musl static binary)
                                       /web/index.html
                                       /web/assets/*
```

**Invariant: the web UI is versioned and deployed atomically with `fah-api`.
The runtime image owns `/web`; `/web` must not be provided by a persistent
volume.** One image is one API-and-UI pair, always. A mounted `/web` breaks
that — a rollback would leave a newer UI calling an older API, and the
mismatch surfaces to the user as unexplained `404`s rather than as a version
problem. The volumes stay exactly `/config` and `/data`.

**A.2 Precompress at build time.** `.gz` and `.br` sit beside each asset; the
handler picks by `Accept-Encoding` and serves the bytes as-is. No runtime
compression, so no CPU on the box for something that never changes.

**Vite does not emit them on its own** — that needs a plugin or a post-build
step, and the frontend-foundation task owns adding it. Stage A ships a small
fixture bundle so the handler's encoding selection, MIME types and cache split
can be tested before any real bundle exists.

**A.3 Handler shape — `ServeDir`, decided.** Serves from `/web` against a fixed
root with no path traversal: match path, pick encoding, set `Content-Type`,
answer `304`. Prefer the well-tested static-file service over saving a little
code size; a hand-rolled handler is evaluated only if measurement shows the
dependency materially regresses the ~30 MB image budget.

Two details the obvious specification gets wrong:

- **`ServeDir` does not generate `ETag`.** Revalidation is `Last-Modified` /
  `If-Modified-Since`. No test asserts an `ETag`, and none is added without a
  demonstrated requirement `Last-Modified` cannot meet.
- **`Vary: Accept-Encoding` is required** wherever a body is chosen by request
  header under a long `public` max-age. Check what `ServeDir` emits rather than
  assuming, and add the header in the response-header layer if it does not.

**A.4 Route order.** `/api/v1` stays nested and answers first, so an unknown
API path still returns the API's own `404` rather than the SPA shell. The SPA
fallback catches only non-API paths. `/health` unchanged.

**A.5 Auth exemption — a closed allowlist.** Static routes are exempt from the
auth layer, as `/health` is; the bundle holds no secrets, and gating it means the
login page cannot load. The exemption names `/`, `/assets/*` and the SPA
fallback. It is **not** expressed as "anything not under `/api/v1`" — there is no
other root-mounted route today, which is exactly why the rule is fixed before one
appears.

**A.6 Cache headers.** Content-hashed assets get `immutable` with a long
max-age; `index.html` is revalidatable and never `immutable`, so a new image
delivers a new shell. The shell is checked every time, the assets it points at
are never checked again.

**A.7 `.dockerignore` and layer caching.** `/dashboard` is excluded today and has
to stop being; `dashboard/frontend/node_modules` and `dist` take its place. The
frontend stage copies only `dashboard/frontend/` and is pinned
`--platform=$BUILDPLATFORM` — its output has no architecture, and without that the
arm64 build runs the whole toolchain under emulation. A UI edit must not
invalidate the Rust builder's cache.

**Done when:** a built bundle is served at `/`, `/api/v1/*` is unaffected,
layering and route-coverage tests still pass, and binary-size delta is measured
and recorded.

## Stage A′ — the certificate a browser will accept

Before the session, not after it. The whole cookie design assumes a household
phone will treat this origin as HTTPS and keep a `Secure` `__Host-` cookie on it.

The generated certificate's SANs are `fastadhunter`, `localhost` and
`127.0.0.1` — nothing that matches `https://192.168.x.x:8443/`. SECURITY.md
promises "a one-time warning for the self-signed certificate"; what a browser
actually shows is a **name mismatch**, which is a different interstitial.

The work: measure real desktop and phone behaviour per address form, confirm the
cookie sets *and persists*, pick the smallest SAN mechanism that needs no
hand-edited config, and define what regenerating the certificate does to
exceptions already accepted on household devices.

**Not Phase 3.** Phase 3 brings CA generation, PEM/PFX import and export. This
stage must not grow a second certificate architecture for Phase 3 to collide
with.

## Stage A″ — freeze the API contracts

Two changes the dashboard needs, made before the typed client is written rather
than discovered page by page.

- **`/events` gains a subscription.** One client message, four event names,
  default every event so no existing consumer breaks. Filtering happens at the
  hub, **before per-query publish work** — `has_subscribers()` becomes a
  has-query-subscribers check, so a stats-only dashboard costs the engine what no
  dashboard costs it. Filtering only on the send path would fix the bandwidth and
  leave the engine cost, which on a LAN is the half that matters. No new
  per-client buffering: the existing broadcast capacity and lag-disconnect stay
  the backpressure design.
- **`GET /clients` gains the in-force policy and its assignment source**, so the
  Clients table renders its designed column in one request instead of one per
  row.

New API surface is written into API.md **marked reserved**, following the
`## Certificates *(Phase 3 — reserved)*` precedent already in that file. Reserved
means frozen enough to write a typed client and tests against, and honestly not
yet shipped; the commit that implements a route promotes its section.

## Stage B — authentication

Follows the draft, with the corrections below; nothing invented here.

**B.1 Storage.** Argon2id password hash in persistent configuration; a separate
random session secret in `/data`, generated on first init, never in source
control, permissions restricted.

**B.2 Routes.** Login, logout, logout-all (secret rotation), password change.
Under `/api/v1/`, exempt from the bearer requirement for login only.

**B.3 Cookie.** `__Host-session`, `Secure`, `HttpOnly`, `SameSite=Strict`,
`Path=/`, explicit expiry. Token from a CSPRNG, at least 128 bits. Auth
responses carry `Cache-Control: no-store`.

The cookie's `Expires` is a convenience for the browser. **The authoritative
expiry lives inside the signed token and is enforced server-side** — a client can
simply not honour an attribute.

**B.4 Middleware.** `require_api_key` accepts a valid session cookie **or** a
bearer key. The bearer key stays — it is the contract for non-browser clients
and for `fastadhunter --healthcheck`-style use. Only the browser stops needing
it.

**B.5 WebSocket.** The upgrade accepts the cookie, which retires `?token=` for
the dashboard. A token in a query string lands in logs; the dashboard must
never use that path.

**`Origin` is validated on a cookie-authenticated upgrade.** Browsers send
cookies on the handshake and a WebSocket has no same-origin policy, so
`SameSite=Strict` is defence in depth, not the whole defence — otherwise any LAN
page could open a socket onto the household's query feed. The rule:
cookie-authenticated → `Origin` present and equal to the request's own effective
target origin; bearer-authenticated → `Origin` irrelevant, and absent is normal
for scripts.

**No configured origin allowlist.** The dashboard is same-origin by construction,
and a list would break the moment the box is reached by a different name than the
one configured — the very situation Stage A′ maps. Deriving the target origin
from `Host` is sound only because nothing proxies this listener; that assumption
is recorded, because behind a proxy the check means nothing without a
forwarded-header policy.

**B.6 `401` handling.** Reuse the existing `unauthorized` code rather than
adding one. The error-code set is a published contract, and the UI does not
need to distinguish "expired" from "never had a session" — both return to
login.

**B.7 `auth.*` never travels through `/config`.** `GET` redacts it — the raw
All-settings panel renders whatever that endpoint returns, and an Argon2id hash
is offline-crackable material. `POST` rejects it with `422`, as `rules.lists` and
`policies` are rejected and for the same one-writer reason: a deep-merge patch
could otherwise set a password while bypassing the change route, its
current-password check and session invalidation.

**B.8 Password change requires the current password.** An explicit
reauthentication step for a privileged operation, and the last barrier standing
for an unattended logged-in browser.

**B.9 Rate-limit login, and bound concurrency separately.** Rate limiting answers
online guessing. It does **not** answer memory: peak RSS is driven by *concurrent*
Argon2id verifications, and eight at once at 19 MiB is ~150 MiB of transient
allocation on a 1 GB box shared with RouterOS. A small semaphore bounds that —
`try_acquire`, `503` with `Retry-After` on saturation, never an unbounded wait
queue that lets accepted connections park on one permit.

**B.10 Argon2id runs on `spawn_blocking`.** One multi-thread runtime serves DNS,
HTTP and the API; a verification on a worker thread stalls query answering for
its whole duration on a 4-core box. The precedent is already in the binary.

**B.11 Parameters are measured on the RB5009**, not copied from a guide. Report
login latency *and* peak RSS during verification, converted with the ~9× factor,
against an explicitly stated **transient** peak allowance — the ≤ 128 MB budget
is a steady-state figure and a criterion written against it is not falsifiable.

**Done when:** login sets the cookie, the API accepts it, the WebSocket accepts
it and rejects a foreign origin, `auth.*` is absent from `GET /config` and `422`
on `POST`, password change invalidates every session, and the measured parameters
are recorded with the device and workload.

## Stage C — frontend foundation

Nothing under `dashboard/` is scaffolded until the owner opens the dashboard
phase — the repo layout note says so explicitly. This stage describes what that
scaffolding is, not permission to write it.

- **C.1** Vite + TypeScript + Preact project, static build target. 3–4 chunks,
  module-preload configured explicitly, `.br`/`.gz` generation added here.
- **C.2** Typed API client, hand-written from API.md — one module per resource
  group, one error type from the documented envelope, unknown fields ignored.
  Hand-written rather than generated: there is no OpenAPI document, and writing
  the types is how the contract gets read carefully.
- **C.3** One WebSocket manager for `WS /api/v1/events`: sends its subscription
  on open and after every reconnect, reconnects with backoff, dispatches the
  message types, exposes connection state. Disconnects are expected — slow
  consumers are dropped by design — so the banner is informational, not an error.

  **A rejected upgrade is not a reconnect loop.** A browser `WebSocket` gives no
  status for a failed handshake, so a `401` is indistinguishable from a dropped
  network. After repeated immediate failures, one authenticated REST probe
  classifies it: expired session → login, transport failure → keep backing off,
  server unreachable → say so.
- **C.4** Shell: sidebar, navbar, card, tile, table, chart wrapper, theme
  tokens, responsive breakpoints. This is where the Pi-hole look is reproduced.
- **C.5** Shared bounded refresh for everything the socket does not push —
  `/telemetry`, `/cache`, `/health`. One slow interval, one in-flight request per
  endpoint however many widgets read it. Built once here so no page invents its
  own timer.

  **Shared, not global**: it polls an endpoint only while a mounted page
  subscribes to it, and the last unsubscribe stops that timer.
- **C.6** Route-scoped lifecycle, owned by the shell so no page has to remember
  it: unmount clears timers and releases event types; re-entry serves bounded
  cached data then revalidates; `hidden` stops work at once and closes the socket
  after a grace period; `visible` cancels a pending close or reconnects and
  restores the active route's subscriptions.
- **C.7** Bundle-size gate: the build fails over 150 KB gzip, with brotli
  reported. Measured on every build, not audited occasionally.

**Done when:** an empty shell renders both themes at all three breakpoints,
authenticates, holds a live socket subscribed to what it needs and nothing more,
and the gate passes.

## Stage D — pages, in this order

Each page is done when it reads only from its listed routes, invents nothing,
and renders empty and error states from real API responses.

| # | Page | Reads | Notes |
| --- | --- | --- | --- |
| D.1 | Dashboard | `/stats`, `/history/summary`, `/telemetry`, `/health`, `/cache`, WS `stats` | both tile rows — HTTP labelled *since restart* — primary `permitted`/`blocked` series, donut, upstream bars, four top-N tables, ruleset card. The socket carries half of it; the shared refresh carries the rest |
| D.2 | Lists | `/lists` CRUD + both refresh routes, WS `list_refreshed` | three-way rule partition, `parse_errors`, all five `last_status` values with `degraded` and `rejected` distinct, `rejected` offering `DELETE`-and-re-add, `409` explained |
| D.3 | Clients | `/clients`, `PUT /clients/{ip}`, `/clients/{ip}/policy` | policy column straight from `/clients` — one request, no fan-out; inline rename and assignment; assignment changes are live, and the UI says so |
| D.4 | Policies | `/policies` CRUD, `stats.policies` | recompile warning on create / `lists` change / delete only; 16-policy ceiling |
| D.5 | Custom Rules | `GET\|PUT /rules/user` | document editor, per-line `422` anchoring; no per-rule CRUD |
| D.6 | Rule Tester | `POST /rules/test` | test under a client or a hypothetical policy |
| D.7 | Cache | `/cache`, `/cache/clean`, `telemetry.counters.swr` + `.cache_cleanup` | stage bar, both bounds, stale-purge as an explicit choice, the `freed_bytes`-vs-RSS note |
| D.8 | Performance | `/history/perf` with `fields`, `/config` for `history.enabled` | p50/p99 per class, QPS, RSS; never an average latency; a disabled recorder says so rather than charting empty |
| D.9 | Upstreams | `telemetry.upstreams`, `/health`, `/config` for the strategy | endpoint health and adaptive state; the strategy named, because under `fallback` the zeros mean "no health state", not "fine"; `degraded` explained, not alarmed |
| D.10 | Settings | `/config` GET/POST, `/config/apikey/rotate`, WS `config_changed` | grouped by config section, every field tagged live or restart-required from hand-carried metadata; `rules.lists`, `policies` and `auth.*` absent; `[api]` excluded or gated |
| D.11 | Diagnostics | `/health`, `/debug/memory`, `telemetry.counters.*`, WS `query` | Health, Memory, Live Feed, answer outcomes, shed, HTTP refusals. The only screen that subscribes to `query`, and it drops the subscription on leaving |

D.1 first because it is the screen that proves the shell, the socket and the
chart layer together. D.11 last because the Live Feed is the piece most likely
to need tuning against real event rates.

## Stage E — packaging

- **E.1** Multi-stage build per A.1: `frontend` stage builds, runtime stage
  takes `/web` with `COPY --from=frontend`. No Node, no `node_modules`, no
  source maps in the shipped image.
- **E.2** Image-size and RSS delta measured against a pre-change checkout, on
  the RB5009, recorded under `docs/code-review/` with corpus, workload and
  device.
- **E.3** Startup unchanged — no new service, no new port, no new volume.
  `/web` is image content; the volume set stays `/config` and `/data`.
  A deployment that mounts over `/web` is a misconfiguration, and the handler
  should say so loudly at boot if `/web/index.html` is missing rather than
  serving `404`s for the whole UI.
- **E.4** Verify the appliance renders with no internet reachable. The box may
  be the network's only resolver; a dashboard that needs the internet fails
  exactly when it is needed.
- **E.5** **Certificate experience — re-confirmed, not decided here.** Stage A′
  settled the SAN mechanism and measured browser behaviour before the session was
  built. This stage checks the shipped image still behaves that way and that the
  cookie survives a container restart on the phone.

  **No HTTP fallback**, under any framing. The session cookie is `Secure` and
  `__Host-`-prefixed, so it does not exist over plain HTTP; serving the
  dashboard unencrypted would put a session credential and the whole config
  surface in clear on the LAN. A certificate warning is a deployment problem
  with a deployment fix, never a reason to downgrade the transport — and
  nothing in the UI should be designed to make the warning less visible.

## What this plan refuses to do

- No persisted query log, and no client-side illusion of one.
- No mock data, ever — a screen with no endpoint is not built.
- No second container and no Node runtime in the appliance.
- No database for sessions.
- No Pi-hole name, mark, wording or link in the shipped bundle.
- No feature added because Pi-hole has it.
- No configured origin allowlist, and no config key the household must hand-edit
  to make the dashboard work.
- No API.md section describing a route that does not exist yet as though it does.

## Phase and ownership

The plan lives at `plan/open/phase5/`, ten tasks in the order above. Two owner
decisions of 2026-08-25 govern it:

- **Phase 5 waits for `phase2.6-adaptive-stage1`** to close — it is the active
  `wip` phase — but is **promoted ahead of `phase3` and `phase4`**.
- **Phase 3 and Phase 4 each trigger a follow-up dashboard review**: certificate
  UI and per-client HTTPS interception from Phase 3, the Lists rule partition from
  Phase 4. Nothing is designed around them now.

Documentation edits are **not** batch-approved. Every repository document this
phase touches is listed in the phase `CLAUDE.md`, and each concrete edit still
needs the owner's yes when the task reaches it.

## Still open, none of it blocking

The non-blocking items in [open-questions.md](open-questions.md) — feed ring
size, display timezone, default range, reconnect backoff shape, and where the
Node toolchain sits relative to the workspace quality gates.
