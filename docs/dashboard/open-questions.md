# Open Questions

Blockers and undecided points for the dashboard. Each one needs an answer
before the code it gates is written.

## 1. Authentication — blocking

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

**Interim, if the session work lands later.** The dashboard can ship
key-in-browser, but only with that trade-off written down and the key held in
memory for the tab rather than persisted. This is worse and should not become
the permanent answer.

**Also open.** Whether a session cookie is enough on its own for the mutating
routes, or whether an anti-CSRF measure is needed. `SameSite` covers most of
it; the decision belongs with the draft's token-format question.

## 2. Serving the bundle — blocking

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

**Sub-questions.**

- Where the assets live in the image, given `/config` and `/data` are the
  volumes and the binary is a musl static build in a distroless image —
  probably baked into the image rather than mounted.
- Whether the static route is behind auth at all. The bundle contains no
  secrets, and gating it means the login page cannot load; `/health` is the
  precedent for an exemption.
- Cache headers: the bundle is content-hashed and immutable, `index.html` is
  not.

## 3. Undecided, not blocking

- **Live Feed ring size.** How many events the client keeps. Bounded by design;
  the number should come from a measurement of row-render cost on a phone, not
  a guess.
- **Time zone for display.** The API speaks RFC 3339 UTC and history buckets
  are UTC days. Policies carry their own `schedule.timezone`. Whether charts
  render in browser-local or UTC — and if browser-local, how a UTC day boundary
  in `resolution=day` is labelled without lying.
- **Default dashboard range.** 24 h matches `GET /stats`; 7 d shows more.
- **Reconnect policy for `WS /events`.** Backoff shape and when the UI falls
  back to polling `GET /stats`. Slow consumers are disconnected by design, so
  reconnect is expected traffic, not an error path.
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

### Diagnostics as a top-level section — closed 2026-08-25

**Stays nested under System.** Health, Memory and Live Feed are operational
views, reached while investigating rather than in daily use, so they do not earn
a fourth top-level section. The sidebar keeps four sections; System holds
Settings and Diagnostics.
