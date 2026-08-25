# P5-04 — Dashboard Authentication and Sessions

**Phase:** 5 · **Depends on:** p5-02, p5-03 · **Model:** Opus

## Goal

A browser can sign in with a password and stay signed in through a cookie, on
both the REST surface and the WebSocket. The API key survives unchanged for
scripts. No credential is ever stored in browser storage, and no password
verification is ever allowed to stall DNS answering or to multiply memory.

## Context

Today `require_api_key` wraps the whole router with `/health` exempt, and the
event socket authenticates by `Authorization` header or `?token=` query
parameter. A browser dashboard on that surface would have to keep the API key in
the page — a key that can rotate itself and never expires.

The design is settled in
[docs/dashboard/auth-design-draft.md](../../../docs/dashboard/auth-design-draft.md):
Argon2id hash in persistent config, a separate random session secret in `/data`,
a compact signed cookie with an expiry, no session table. This task closes the
six decisions that draft leaves open and implements the result.

`p5-02` has already established that a `Secure` `__Host-` cookie persists on the
household's real devices against this box's certificate. If it did not, this task
does not start.

`p5-03` has already written the reserved API.md contract this task implements and
promotes to live wording.

**No hand-rolled crypto** (root CLAUDE.md hard rule 5). Vetted crates for
Argon2id and for whatever primitive signs the token.

## Scope

### Decisions to close and record in the review file

1. Argon2id parameters, **measured on the RB5009** — report login latency and
   peak RSS during verification, converting dev figures with the ~9× factor
   rather than assuming.
2. Session lifetime, and whether an inactivity timeout is added to absolute
   expiry. If sliding, say what re-issuing a cookie on every response costs.
3. Token format and the signing/authentication primitive.
4. Secret generation and first-run behaviour: what happens on a box with no
   password set yet. **An open first-visit setup page is not the answer** — on a
   LAN, whoever reaches it first owns the box. The API key's own precedent is
   available: generate at first boot, print once to the container log.
5. Password change, and how it invalidates every existing session.
6. Whether `/data` needs revocation state beyond global secret rotation (default
   answer: no).

### Argon2id — how it runs, not just what it costs

- **`spawn_blocking`, always.** One multi-thread Tokio runtime serves DNS, HTTP
  and the API (`crates/fastadhunter/src/main.rs:242`). A verification on a worker
  thread stalls query answering on a 4-core box for its whole duration. The
  precedent and the reasoning already exist at `main.rs:757` — follow them.
- **A concurrency bound, separate from the rate limit.** Peak RSS is driven by
  *concurrent* verifications, not by their rate: eight at once at 19 MiB is
  ~150 MiB of transient allocation. A small semaphore bounds it — **start at 1–2
  permits and measure before raising**.
- **`try_acquire`, not an unbounded wait.** On saturation answer `503` with
  `Retry-After`. Queueing means 64 accepted connections can park on one permit
  and hold tasks, file descriptors and connection slots for the duration.
- **Rate limiting stays**, bounded and in-memory, as a distinct control against
  online guessing.

### Storage, routes, cookie

- **Storage**: Argon2id hash in the persistent config; session secret in `/data`,
  generated on first init, permissions restricted, never logged, never returned
  by `GET /config`.
- **Routes**: login, logout, logout-everywhere, password change. Login is exempt
  from the API-key requirement; the rest are not.
- **Password change requires the current password.** With `SameSite=Strict` and
  no CSRF token, that check is the remaining barrier for an unattended
  logged-in browser, and it is the reauthentication step a privileged operation
  should carry regardless.
- **Cookie**: `__Host-` prefix, `Secure`, `HttpOnly`, `SameSite=Strict`,
  `Path=/`, explicit expiry. Token from a CSPRNG, ≥ 128 bits. Auth responses
  carry `Cache-Control: no-store`.
- **The authoritative expiry lives inside the signed token** and is enforced
  server-side. The cookie's `Expires`/`Max-Age` is a client-side convenience and
  is not the security boundary — a browser can simply not honour it.
- **Middleware**: `require_api_key` accepts a valid session cookie **or** a
  bearer key. The bearer path is unchanged for existing clients.
- **`401` handling**: reuse the existing `unauthorized` code. The published
  error-code set is a contract, and the UI does not need to tell "expired" from
  "never had a session" — both return to login.
- **Failure messages reveal nothing** about which half was wrong.

### `auth.*` must not travel through `/config`

`routes.rs:1237` returns the whole `fah_config::Config`, and its comment at
`:1233` asserts "Nothing here is secret … so 'secrets redacted' holds by
construction". Adding `[auth]` breaks that invariant in two directions:

- **`GET /config` redacts or omits every `auth.*` field.** The Argon2id hash must
  never reach a response body. That it is a hash is not a licence to publish it —
  it is offline-crackable material, and `p5-09`'s read-only *All settings* panel
  renders whatever `GET /config` returns, straight into a browser. Update that
  handler's doc comment too: the invariant it states will no longer be true by
  construction, only by enforcement.
- **`POST /config` rejects `auth.*` with `422`**, exactly as `rules.lists` and
  `policies` are rejected, and for the same one-writer reason. Without it, a
  deep-merge patch sets a password hash while bypassing the password-change
  route, its current-password check and global session invalidation.

### WebSocket

- The upgrade accepts the cookie. `?token=` keeps working for non-browser clients
  but the dashboard must never use it — a token in a query string lands in logs.
- **`Origin` is validated on a cookie-authenticated upgrade.** Browsers send
  cookies on the handshake and a WebSocket has no same-origin policy, so
  `SameSite` is defence in depth rather than the whole defence. The rule:
  - cookie-authenticated → `Origin` must be present and match the request's own
    effective target origin (scheme + host + port, from `Host` or h2
    `:authority`);
  - bearer-authenticated → `Origin` is irrelevant and its absence is normal for
    scripts.
- **No configured origin allowlist.** The dashboard is same-origin by
  construction; a list of trusted origins would break the moment the operator
  reaches the box by a different name than the one configured — which is exactly
  the situation `p5-02` maps — and would violate the rule that every key ships
  with a working compiled-in default.
- **Deriving the origin from `Host` is safe only because nothing proxies this
  listener.** Record that assumption in the review file: behind a reverse proxy
  `Host` becomes attacker-influenced and this check needs a forwarded-header
  policy to mean anything.

### Docs and gates

- **Promote `p5-03`'s reserved API.md sections to live wording** in this change,
  and add the `auth.*` rules on `/config`.
- SECURITY.md gains the session model and its trade-off; CONFIGURATION.md gains
  the `[auth]` section and its mutability class. **Each is a repository contract
  document needing the owner's explicit yes for that concrete change** (root
  CLAUDE.md §Working agreement 1) — propose, then wait.
- **`request_coverage.rs`**: login, logout, logout-everywhere and password change
  are new routes and need request fixtures under `requests/`, or an `UNCOVERED`
  entry with the reason. Login and logout take fixtures; a destructive
  logout-everywhere may be better as an allowlist entry — decide and say which.

## Acceptance criteria

- Correct password sets the cookie; wrong password does not, and the two
  responses are indistinguishable beyond success/failure.
- A cookie-authenticated request succeeds on `/api/v1/*`; an expired one gets
  `401` with the documented envelope. Expiry is enforced from the token, proven
  by a cookie whose client-side `Expires` has been extended.
- The event socket upgrades with only the cookie present, **and rejects a
  cookie-authenticated upgrade carrying a foreign `Origin`**. Accepted and
  rejected origins both tested; a bearer upgrade with no `Origin` still succeeds.
- Bearer-key access is unchanged — existing tests still pass untouched.
- Password change requires the current password and invalidates every existing
  session (test: an old cookie stops working).
- `GET /config` contains no `auth.*` material — asserted by test on the response
  body, not by reading the struct.
- `POST /config` carrying `auth.password_hash` returns `422`.
- The session secret never appears in `GET /config`, in logs, or in any error.
- Argon2id runs on `spawn_blocking` — proven by a test or a measurement showing
  DNS latency unmoved while a verification is in flight.
- **Concurrent logins cannot multiply Argon2id memory past a stated transient
  peak allowance.** Record that allowance explicitly and separately from the
  ≤ 128 MB steady-state budget: PERFORMANCE.md §Budgets treats a peak as a
  transient, and a criterion written against the steady-state row is not
  falsifiable. Prove the semaphore bounds concurrent verifications and that
  saturation answers `503` with `Retry-After`.
- Rate limiting is bounded in memory and proven to reject a burst.
- Argon2id parameters recorded with device, workload and measured peak RSS.
- Gates green, `request_coverage.rs` included.

## Out of scope

Multiple users, roles, or per-session revocation. TLS client certificates.
Certificate or SAN work (`p5-02`). Anything the frontend renders (`p5-05`).

## Suggested prompt

> Read root CLAUDE.md hard rules, SECURITY.md, API.md,
> docs/dashboard/auth-design-draft.md, the p5-02 review file, and
> plan/open/phase5/p5-04-auth-session.md. Close the six open decisions with
> reasoning, implement Argon2id storage on `spawn_blocking` behind a small
> `try_acquire` semaphore, the session secret, the cookie with server-side expiry
> in the token, the auth routes with a current-password check, cookie acceptance
> on REST and on the WebSocket upgrade with same-origin validation, the `auth.*`
> redaction and `422` on `/config`, and login rate limiting. Measure Argon2id on
> the RB5009 and record latency and peak RSS against a stated transient
> allowance. Propose the API.md / SECURITY.md / CONFIGURATION.md edits and wait
> for approval before making them.
