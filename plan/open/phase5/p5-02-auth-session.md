# P5-02 — Dashboard Authentication and Sessions

**Phase:** 5 · **Depends on:** p5-01 · **Model:** Opus

## Goal

A browser can sign in with a password and stay signed in through a cookie, on
both the REST surface and the WebSocket. The API key survives unchanged for
scripts. No credential is ever stored in browser storage.

## Context

Today `require_api_key` wraps the whole router with `/health` exempt, and the
event socket authenticates by `Authorization` header or `?token=` query
parameter. A browser dashboard on that surface would have to keep the API key
in the page — a key that can rotate itself and never expires.

The design is settled in
[docs/dashboard/auth-design-draft.md](../../../docs/dashboard/auth-design-draft.md):
Argon2id hash in persistent config, a separate random session secret in `/data`,
a compact signed cookie with an expiry, no session table. This task closes the
six decisions that draft leaves open and implements the result.

**No hand-rolled crypto** (root CLAUDE.md hard rule 5). Use vetted crates for
Argon2id and for whatever primitive signs the token.

## Scope

- **Decisions to close and record in the review file**, with reasoning:
  1. Argon2id parameters, **measured on the RB5009** — report login latency and
     peak RSS during verification, and convert dev figures with the ~9× factor
     rather than assuming.
  2. Session lifetime, and whether an inactivity timeout is added to absolute
     expiry.
  3. Token format and the signing/authentication primitive.
  4. Secret generation and first-run behaviour: what happens on a box with no
     password set yet.
  5. Password change, and how it invalidates every existing session.
  6. Whether `/data` needs revocation state beyond global secret rotation
     (default answer: no).
- **Storage**: Argon2id hash in the persistent config; session secret in
  `/data`, generated on first init, permissions restricted, never logged, never
  returned by `GET /config`.
- **Routes**: login, logout, logout-everywhere, password change. Login is
  exempt from the API-key requirement; the rest are not.
- **Cookie**: `__Host-` prefix, `Secure`, `HttpOnly`, `SameSite=Strict`,
  `Path=/`, explicit expiry. Token from a CSPRNG, ≥ 128 bits. Auth responses
  carry `Cache-Control: no-store`.
- **Middleware**: `require_api_key` accepts a valid session cookie **or** a
  bearer key. The bearer path is unchanged for existing clients.
- **WebSocket**: the upgrade accepts the cookie. `?token=` keeps working for
  non-browser clients but the dashboard must never use it — a token in a query
  string lands in logs.
- **Rate limiting on login**, bounded and in-memory. Required by the draft, and
  doubly so here: each Argon2id verification allocates its memory parameter, so
  unthrottled attempts are a memory-pressure vector on a 1 GB box shared with
  RouterOS.
- **Failure messages reveal nothing** about which half was wrong.
- **`401` handling**: reuse the existing `unauthorized` code. The published
  error-code set is a contract, and the UI does not need to tell "expired" from
  "never had a session" — both return to login.
- **Doc updates in the same change**: API.md (routes, cookie, what the socket
  accepts), SECURITY.md (the session model and its trade-off), CONFIGURATION.md
  (the `[auth]` section and its mutability class).

## Acceptance criteria

- Correct password sets the cookie; wrong password does not, and the two
  responses are indistinguishable beyond success/failure.
- A cookie-authenticated request succeeds on `/api/v1/*`; an expired one gets
  `401` with the documented envelope.
- The event socket upgrades with only the cookie present.
- Bearer-key access is unchanged — existing tests still pass untouched.
- Password change invalidates every existing session (test: an old cookie stops
  working).
- The session secret never appears in `GET /config`, in logs, or in any error.
- Rate limiting is bounded in memory and proven to reject a burst.
- Argon2id parameters recorded with device, workload and measured peak RSS.
- Gates green.

## Out of scope

Multiple users, roles, or per-session revocation. TLS client certificates.
Anything the frontend renders (`p5-03`).

## Suggested prompt

> Read root CLAUDE.md hard rules, SECURITY.md, API.md,
> docs/dashboard/auth-design-draft.md, and
> plan/wip/phase5/p5-02-auth-session.md. Close the six open decisions with
> reasoning, implement Argon2id storage, the session secret, the cookie, the
> auth routes, cookie acceptance on REST and the WebSocket upgrade, and login
> rate limiting. Measure the Argon2id parameters on the RB5009 and record
> latency and peak RSS. Update API.md, SECURITY.md and CONFIGURATION.md in the
> same change.
