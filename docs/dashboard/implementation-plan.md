# Implementation Plan

Derived from the finalized design decisions. The source of truth for *what* is
built is [capability-matrix.md](capability-matrix.md) and
[information-architecture.md](information-architecture.md); for *how it looks*,
[visual-system.md](visual-system.md). This file is the order of work.

Two backend contracts gate the frontend. Both are verified in code before any
page is written, so no page is built against an assumption that later forces a
rewrite.

## Frozen decisions

- TypeScript + Preact + Vite. Static bundle only. No SSR, no Node runtime in
  the appliance.
- uPlot for time series, hand-drawn SVG for donuts.
- Own CSS. No AdminLTE, Bootstrap, jQuery, DataTables.
- Under 150 KB gzip, whole bundle. No CDN.
- Pi-hole is the visual and interaction reference, never the domain model.
- One origin: `fah-api` serves the bundle from `/web`, baked into the runtime
  image by a multi-stage build. The UI and the API ship and version as one
  artifact; `/web` is never a volume.
- Session cookie auth per
  [auth-design-draft.md](auth-design-draft.md). No API key in
  `localStorage` as the normal mechanism.

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

## Stage 0 — verify the contracts (no frontend code)

**0.1 Static serving.** Verified 2026-08-25: `fah-api` serves no static files.
`router()` is `/health` + `.nest("/api/v1", …)` + `.fallback(not_found)`,
wrapped in `require_api_key`. No `ServeDir`, no `ServeFile`, no `tower-http` in
the crate. Nothing to reuse; Stage A builds it.

**0.2 Auth.** Verified: bearer-only. `require_api_key` layers the whole router
with `/health` exempt. No login route, no cookie, no session. The WebSocket
authenticates by `Authorization` header or `?token=` query parameter.

**0.3 Close the six open decisions** in the auth draft — Argon2id parameters,
session lifetime and inactivity policy, token format and signing primitive,
secret generation and first-run behaviour, password-change and global
invalidation, and whether `/data` revocation state is needed. These are backend
decisions; the frontend only needs the resulting routes and semantics.

**0.4 Doc updates** (needs the owner's go, separately): API.md gains the auth
routes, the cookie mechanism and the static routes; SECURITY.md gains the
session model; CONFIGURATION.md gains `[auth]`.

**Done when:** the auth routes, cookie attributes and static routes are written
down in API.md, and the frontend can be built against a stable contract.

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

**A.2 Precompress at build time.** Vite emits `.gz` and `.br` alongside each
asset; the handler picks by `Accept-Encoding` and serves the bytes as-is. No
runtime compression, so no CPU on the box for something that never changes.

**A.3 Handler shape.** Serves from `/web`, resolving each request against a
fixed root with no path traversal: match path, pick encoding, set
`Content-Type` and an `ETag`, answer `304`. Whether that is `tower-http`'s
`ServeDir` or a hand-rolled handler is a dependency-weight call to make when
the code is written — `ServeDir` is correct and battle-tested, and the whole
need is small enough to write directly. Decide by measuring the dependency's
cost against the appliance's binary-size budget, not by preference.

**A.4 Route order.** `/api/v1` stays nested and answers first, so an unknown
API path still returns the API's own `404` rather than the SPA shell. The SPA
fallback catches only non-API paths. `/health` unchanged.

**A.5 Auth exemption.** Static routes are exempt from the auth layer, as
`/health` is. The bundle holds no secrets, and gating it means the login page
cannot load.

**A.6 Cache headers.** Content-hashed assets get `immutable` with a long
max-age; `index.html` gets `no-cache`.

**Done when:** a built bundle is served at `/`, `/api/v1/*` is unaffected,
layering tests still pass, and binary-size delta is measured and recorded.

## Stage B — authentication

Follows the draft; nothing invented here.

**B.1 Storage.** Argon2id password hash in persistent configuration; a separate
random session secret in `/data`, generated on first init, never in source
control, permissions restricted.

**B.2 Routes.** Login, logout, logout-all (secret rotation), password change.
Under `/api/v1/`, exempt from the bearer requirement for login only.

**B.3 Cookie.** `__Host-session`, `Secure`, `HttpOnly`, `SameSite=Strict`,
`Path=/`, explicit expiry. Token from a CSPRNG, at least 128 bits. Auth
responses carry `Cache-Control: no-store`.

**B.4 Middleware.** `require_api_key` accepts a valid session cookie **or** a
bearer key. The bearer key stays — it is the contract for non-browser clients
and for `fastadhunter --healthcheck`-style use. Only the browser stops needing
it.

**B.5 WebSocket.** The upgrade accepts the cookie, which retires `?token=` for
the dashboard. A token in a query string lands in logs; the dashboard must
never use that path.

**B.6 `401` handling.** Reuse the existing `unauthorized` code rather than
adding one. The error-code set is a published contract, and the UI does not
need to distinguish "expired" from "never had a session" — both return to
login.

**B.7 Rate-limit login.** Required by the draft, and doubly so here: each
Argon2id verification allocates its memory parameter. On a 1 GB box shared with
RouterOS, unthrottled attempts are a memory-pressure vector, not just a
guessing one.

**B.8 Argon2id parameters are measured on the RB5009**, not copied from a
guide. Report login latency *and* peak RSS during verification, and convert dev
figures with the ~9× factor rather than assuming.

**Done when:** login sets the cookie, the API accepts it, the WebSocket accepts
it, password change invalidates every session, and the measured parameters are
recorded with the device and workload.

## Stage C — frontend foundation

Nothing under `dashboard/` is scaffolded until the owner opens the dashboard
phase — the repo layout note says so explicitly. This stage describes what that
scaffolding is, not permission to write it.

- **C.1** Vite + TypeScript + Preact project, static build target.
- **C.2** Typed API client, hand-written from API.md — one module per resource
  group, one error type from the documented envelope, unknown fields ignored.
  Hand-written rather than generated: there is no OpenAPI document, and writing
  the types is how the contract gets read carefully.
- **C.3** One WebSocket manager for `WS /api/v1/events`: reconnect with
  backoff, dispatch `query` / `stats` / `config_changed` / `list_refreshed`,
  expose connection state. Disconnects are expected — slow consumers are
  dropped by design — so the banner is informational, not an error.
- **C.4** Shell: sidebar, navbar, card, tile, table, chart wrapper, theme
  tokens, responsive breakpoints. This is where the Pi-hole look is reproduced.
- **C.5** Bundle-size gate: the build fails over 150 KB gzip. Measured on every
  build, not audited occasionally.

**Done when:** an empty shell renders both themes at all three breakpoints,
authenticates, holds a live socket, and the gate passes.

## Stage D — pages, in this order

Each page is done when it reads only from its listed routes, invents nothing,
and renders empty and error states from real API responses.

| # | Page | Reads | Notes |
| --- | --- | --- | --- |
| D.1 | Dashboard | `/stats`, `/history/summary`, `/telemetry`, WS `stats` | both tile rows, primary series, donut, upstream bars, four top-N tables, ruleset card |
| D.2 | Lists | `/lists` CRUD + both refresh routes, WS `list_refreshed` | three-way rule partition, `last_status` / `last_error` surfaced, `409` explained |
| D.3 | Clients | `/clients`, `PUT /clients/{ip}`, `/clients/{ip}/policy` | inline rename and assignment; assignment changes are live, and the UI says so |
| D.4 | Policies | `/policies` CRUD, `stats.policies` | recompile warning on create / `lists` change / delete only; 16-policy ceiling |
| D.5 | Custom Rules | `GET\|PUT /rules/user` | document editor, per-line `422` anchoring; no per-rule CRUD |
| D.6 | Rule Tester | `POST /rules/test` | test under a client or a hypothetical policy |
| D.7 | Cache | `/cache`, `/cache/clean`, `telemetry.counters.swr` + `.cache_cleanup` | stage bar, both bounds, stale-purge as an explicit choice, the `freed_bytes`-vs-RSS note |
| D.8 | Performance | `/history/perf` with `fields` | p50/p99 per class, QPS, RSS; never an average latency |
| D.9 | Upstreams | `telemetry.upstreams`, `/health` | endpoint health and adaptive state; `degraded` explained, not alarmed |
| D.10 | Settings | `/config` GET/POST, `/config/apikey/rotate`, WS `config_changed` | grouped by config section, every field tagged live or restart-required; `rules.lists` and `policies` absent |
| D.11 | Diagnostics | `/health`, `/debug/memory`, `telemetry.counters.*`, WS `query` | Health, Memory, Live Feed, answer outcomes, shed, HTTP refusals |

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
- **E.5** **Certificate experience, as a deployment concern.** The dashboard
  inherits the API's TLS listener and its certificate; on a household box that
  is the box's own, so first visit shows a browser warning — and phone browsers
  are stricter about it than desktop ones. Record what each household browser
  shows and what it takes to proceed, then recommend a path (accept once,
  install the CA on household devices, or a trusted name and certificate) in
  SECURITY.md and the deployment notes.

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

## Owner decisions still outstanding

1. Opening a dashboard phase under `plan/` — `plan/open/` currently holds
   `phase3` and `phase4` only, and nothing under `dashboard/` is scaffolded
   until a phase says so.
2. The doc updates in 0.4, each of which needs its own go.
3. The six auth decisions in the draft (0.3).
4. The non-blocking items in [open-questions.md](open-questions.md) — feed ring
   size, display timezone, default range, reconnect policy, generated vs
   hand-written config form, where the Node toolchain sits relative to the
   workspace quality gates.
