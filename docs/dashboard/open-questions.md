# Open Questions

Blockers and undecided points for the dashboard. Each one needs an answer
before the code it gates is written.

**Sections 1 and 2 are closed.** They are kept in full because they record what
was weighed; the answers now live in `plan/open/phase5/` and in
[implementation-plan.md](implementation-plan.md), and the closing notes below
each one say where. Section 3 is what is genuinely still open.

## 1. Authentication — closed 2026-08-25, implemented in `p5-04`

**State.** The API accepts a single bearer API key and nothing else.
`crates/fah-api/src/routes.rs` wraps the whole router in `require_api_key`,
with `/health` as the only exemption. There is no login route, no session, no
cookie.

**Problem.** A browser dashboard on a bearer-only API has to hold the API key
in the browser. That key is the whole API — including
`POST /config/apikey/rotate`. Storing it in `localStorage` puts it in reach of
anything that can run script on the origin, and it never expires.

**Direction.** [auth-design-draft.md](auth-design-draft.md)
already proposes the answer: an Argon2id password hash in persistent config, a
random session secret in `/data`, and a compact authenticated session cookie
with an expiry — no database. That draft lists six decisions still open
(Argon2id parameters measured on the target device, session lifetime and
whether inactivity timeout is added to absolute expiry, token format and
signing primitive, secret generation and first-run behaviour, password change
with global session invalidation, and whether `/data` revocation state is
needed beyond secret rotation).

**What the dashboard needs from it.**

- a login route accepting the password and setting the cookie
- a logout route clearing it
- cookie auth accepted alongside bearer on `/api/v1/`, including the WebSocket
  upgrade — `WS /api/v1/events` currently authenticates by header or `?token=`,
  and a token in a query string lands in logs
- `401` distinguishable as "session expired" so the UI can return to login
  rather than showing an error

**Interim, if the session work lands later.** Rejected. `p5-04` precedes every
page, so key-in-browser is never shipped even temporarily.

**CSRF — closed.** `SameSite=Strict` is defence in depth, not the whole defence:
browsers send cookies on a WebSocket handshake and a WebSocket has no
same-origin policy, so any LAN page could otherwise open a socket onto the query
feed. A cookie-authenticated upgrade requires `Origin` to equal the request's own
effective target origin; a bearer-authenticated one does not, and its absence is
normal for scripts. **No configured allowlist** — the dashboard is same-origin by
construction, and a list would fail the moment the box is reached by a name other
than the configured one.

**Also closed with it.** `auth.*` is redacted from `GET /config` and `422` on
`POST /config`; Argon2id runs on `spawn_blocking` behind a `try_acquire`
semaphore; the authoritative session expiry lives inside the signed token;
password change requires the current password. Rationale in
[implementation-plan.md](implementation-plan.md) §Stage B.

## 2. Serving the bundle — closed 2026-08-25, implemented in `p5-01`

**State.** Verified 2026-08-25: `fah-api` serves no static files. The router is
`/health`, `/api/v1/*`, and a `not_found` fallback. There is no `ServeDir`, no
`ServeFile`, and `tower-http` does not appear in the crate.

**Options.**

1. **Serve from `fah-api`.** A static route plus SPA fallback, exempted from
   `require_api_key` the way `/health` is, reading from a directory in the
   image. One port, one process, one TLS setup, one origin — so the session
   cookie is same-origin and no CORS exists. Costs a dependency and a small
   amount of always-resident memory for the handler.
2. **A second container.** Keeps `fah-api` untouched but adds a second image
   and process to a 1 GB box shared with RouterOS, plus cross-origin cookie and
   CORS handling.

**Recommendation: option 1.** Option 2's only advantage is not touching
`fah-api`, and it pays for that with a whole second container on the tightest
resource on the device.

**Sub-questions — all answered.**

- **Where the assets live:** baked into the image at `/web` by a multi-stage
  build. Never a volume; the volume set stays `/config` and `/data`.
- **Auth on the static route:** exempt, as `/health` is — but by a **closed
  positive allowlist** (`/`, `/assets/*`, SPA fallback), never by "anything
  outside `/api/v1`".
- **Cache headers:** hashed assets `immutable` with a long max-age; `index.html`
  revalidatable. Revalidation is `Last-Modified` / `If-Modified-Since` —
  **`ServeDir` emits no `ETag`** and none is added. `Vary: Accept-Encoding` is
  required wherever the body is picked by `Accept-Encoding`.

**One thing this section did not anticipate:** `.dockerignore` excludes
`/dashboard`, so the frontend stage cannot see its own source. `p5-01` fixes it.

## 3. Undecided, not blocking

- **Live Feed ring size.** How many events the client keeps. Bounded by design;
  the number should come from a measurement of row-render cost on a phone, not
  a guess.
- **Time zone for display.** The API speaks RFC 3339 UTC and history buckets
  are UTC days. Policies carry their own `schedule.timezone`. Whether charts
  render in browser-local or UTC — and if browser-local, how a UTC day boundary
  in `resolution=day` is labelled without lying.
- **Default dashboard range.** 24 h matches `GET /stats`; 7 d shows more.
- **Reconnect backoff shape for `WS /events`**, and when the UI falls back to
  polling `GET /stats`. Slow consumers are disconnected by design, so reconnect
  is expected traffic, not an error path. *Partly settled:* an upgrade rejected
  for authentication is classified by a REST probe rather than retried forever
  (`p5-05`), and a socket that does not subscribe to `query` no longer lags on
  query volume at all (`p5-03`). What remains open is only the timing curve.
- **Where the dashboard's own build lives** in the repo layout, and whether its
  Node toolchain is part of the workspace's quality gates or separate.

## 4. Closed

Kept here rather than deleted, so a later reader sees what was weighed instead
of re-opening it. The behaviour each one settles is written up in
[information-architecture.md](information-architecture.md).

### Config form generality — closed 2026-08-25

**Hand-written per config section.** `GET /config` supplies current effective
values and validation metadata; the frontend owns grouping, descriptions,
mutability labelling and which keys appear.

A generated form was the cheaper option and was rejected: it cannot write help
text, cannot know that `rules.lists` belongs on another page, and cannot
distinguish a boot-only key from a live one without being told — which is the
whole job of this page.

Two rules follow from it, and neither is optional:

- **A read-only All settings panel sits at the bottom**, rendering the complete
  effective config. A curated form is a subset by construction, and without the
  raw panel an operator cannot tell "not exposed here" from "not set". The panel
  is what makes the curated decision safe.
- **`POST /config` receives only changed keys.** It is a partial deep-merge.
  Submitting the whole read-back document would overwrite keys the UI does not
  model — a hand-edited TOML value, or one from a newer build — with whatever
  the form last saw. The form therefore tracks dirty state per field rather than
  diffing against a re-fetch, since `config_changed` can move the server's copy
  in between.

One correction to it: **`GET /config` supplies current effective values and
nothing else.** It is not a schema endpoint — no types, no bounds, no enums, no
mutability classes. All of that is hand-carried from CONFIGURATION.md and the
backend schema. That is the real cost of choosing a hand-written form, and it is
paid on purpose.

### Diagnostics as a top-level section — closed 2026-08-25

**Stays nested under System.** Health, Memory and Live Feed are operational
views, reached while investigating rather than in daily use, so they do not earn
a fourth top-level section. The sidebar keeps four sections; System holds
Settings and Diagnostics.

### Bundle shape — closed 2026-08-25

**3–4 chunks, no web font.** "One JS chunk" was the earlier decision and it put
uPlot on the login page and made any edit invalidate the whole immutable asset.
Route-level splitting is free in Vite; unbounded splitting is not, so the count
is bounded and module-preload is configured explicitly. The web font was dropped
for a system stack — 15–40 KB already-compressed and a critical-path request, out
of a 150 KB budget, for no functional gain.

### The `/events` firehose — closed 2026-08-25

**The socket subscribes.** Every connected socket previously received every query
event, and `has_subscribers()` turned on engine-side per-query publish work for
any socket at all — so a phone showing Settings cost bandwidth, battery and
engine time. `p5-03` adds a `subscribe` message, defaulting to every event so no
existing client breaks, with server-side filtering and the engine-side gate moved
to match. Only the Live Feed asks for `query`.

No new per-client buffering: the existing broadcast capacity and lag-disconnect
remain the backpressure design, and filtering is what stops a stats-only socket
lagging on query volume in the first place.
