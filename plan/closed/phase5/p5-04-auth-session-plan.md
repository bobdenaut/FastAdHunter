# P5-04 — Implementation Plan: Dashboard Authentication and Sessions

**Task:** [p5-04-auth-session.md](p5-04-auth-session.md) · **Depends on:** p5-02, p5-03
**Status:** approved plan, revision 3, not implemented. No open decisions.

Revision history:

- **r1**, 2026-08-26 — reconciliation raised seven blockers (B1–B7); the owner
  closed them. §0.1 records those answers.
- **r2**, 2026-08-26 — independent plan-vs-task validation raised eighteen
  findings (F1–F18). Five needed an owner decision (D1–D5, §0.2); twelve are
  plan corrections (E1–E12) folded into the sections below; two were downgraded
  to clarity-only.
- **r3**, 2026-08-26 — three points raised against D1/D2/D4 were accepted:
  D2-a (no `Retry-After` on the TLS-off `503`), D2-b (only `login` is gated on
  TLS), D4-a (`test-harness` cargo feature as the enforcement mechanism).
  §0.3 records them and every affected section is reconciled.

## 0. Decisions closed by the owner

### 0.1 First round — B1–B7

| # | Decision |
| - | -------- |
| 1 | **B2** — the Argon2id hash lives in a separate persistent `/config/auth-hash` file. No `[auth]` section is added to `Config` or `fastadhunter.toml`. Rollback to 0.2.20 stays clean. |
| 2 | **B3** — dev-box measurement plus an owner-run RB5009 leg. **No deployment to the RB5009 while the p2.6 soak is active.** Repeated-login RSS / high-water behaviour is measured, not only first-run latency. |
| 3 | **B4** — `argon2` is approved as the only new runtime crypto dependency. Release-binary and image delta measured explicitly. |
| 4 | **B5** — frozen status semantics: `401` authentication failure · `422` `auth.*` config mutation rejected · `429` login rate-limit rejection · `503` login unavailable. **Narrowed in r3**, superseding "both `429` and `503` carry `Retry-After`": `429 rate_limited` carries `Retry-After`; `503 unavailable` from Argon2 saturation carries `Retry-After: 1`; `503 unavailable` from `api.tls = false` carries **no** `Retry-After`. Presence of the header is the runtime discriminator between the transient and the persistent case. |
| 5 | **B7** — password change emits the existing `config_changed` event; the payload does not change. |
| 6 | First boot with no password generates a random one, prints it once, and persists only the Argon2id hash. Plaintext is never persisted or logged afterwards. |
| 7 | Session expiry is **absolute**; the token's expiry is authoritative. No sliding renewal. |
| 8 | `Origin` validation compares effective scheme + host + port. Cookie-authenticated WebSocket upgrades require same-origin; bearer upgrades are unchanged. |
| 9 | The authentication method is explicit in the request context, so the `Origin` check applies to cookie authentication only. |

**Two clarifications, binding, folded into the sections below:**

| # | Clarification | Where |
| - | ------------- | ----- |
| A | The login contract is `POST /api/v1/auth/login`, request `{"password":"…"}`, response `204 No Content` + `Set-Cookie`. **The generated first-boot password is never returned by the API** — the log line is its only channel. | §5.1 |
| B | An out-of-band password reset — deleting `/config/auth-hash` and letting the next boot regenerate — **must also rotate `/data/session-secret` before the next successful login**, so sessions issued before the reset cannot survive it. | §3.1 |

### 0.2 Second round — D1–D5

| # | Decision |
| - | -------- |
| **D1** | Error codes `rate_limited` (429) and `unavailable` (503) are approved, and API.md's closed code set expands to eight: `bad_request` (400), `unauthorized` (401), `not_found` (404), `conflict` (409), `validation_failed` (422), `rate_limited` (429), `unavailable` (503), `internal` (500). `ApiError` gains the two variants and a per-variant header list, so `Retry-After` rides the existing envelope without changing its shape. |
| **D2** | With `api.tls = false`, `POST /api/v1/auth/login` is rejected with `503` `unavailable`, **no `Retry-After`**, and no `Set-Cookie`, plus a startup `warn!`. The error message states that session authentication requires TLS. `api.tls` is a **boot key**, so the condition persists until a restart with changed configuration and must not advertise a retry interval. The same `unavailable` slug is reused — no extra status or code is added for this case. This is not a configuration error and is never represented as `422`. Bearer authentication is unaffected, and **only `login` is gated**: `logout`, `logout-all` and `password` stay usable over bearer, with `logout-all` still rotating the secret and `password` still requiring the current password. |
| **D3** | p5-04 may close as **PASS WITH DEFERRED FINDINGS** with the RB5009 measurement leg carried forward as a live acceptance item. It is never satisfied by conversion. The ~9× factor estimates latency only; RSS is never converted. Nothing touches the RB5009 while the p2.6 soak is active. |
| **D4** | The rate limiter takes constructor-injected limits for tests and measurement only, behind a **non-default `test-harness` cargo feature** that also carries §10's in-memory `AuthState` constructor. Production constants are fixed at 5 attempts / 60 s / source address, 30 / 60 s global, 128 tracked addresses. Not a config key, not an env var, not runtime-configurable, not exposed through the API. Production `AuthState` always uses the production constants, and the feature is never enabled in the shipped release build. `#[cfg(test)]` is explicitly **not** the mechanism — the external integration crates link `fah-api` as a dependency and never see crate-local `cfg(test)`. |
| **D5** | The documentation set in §14 is approved, including that the task's own sentence *"CONFIGURATION.md gains the `[auth]` section and its mutability class"* is superseded by decision 1 and is corrected rather than silently ignored. |

### 0.3 Third round — D2-a, D2-b, D4-a, all resolved

Three points were raised against D1/D2/D4 in r2 and closed by the owner in r3.
They are folded into the sections below; nothing here remains open.

| # | Raised | Resolution |
| - | ------ | ---------- |
| **D2-a** | `Retry-After: 1` on the TLS-off `503` advertises a retry for a condition that never clears without a restart, and makes the transient and persistent `503` cases indistinguishable. | **Accepted.** The TLS-off `503` carries no `Retry-After`; the saturation `503` remains the only one that carries `Retry-After: 1`. Same `unavailable` slug for both. §0.1 decision 4 narrowed accordingly. |
| **D2-b** | D2 covered `login` only, leaving the other three routes' behaviour under `api.tls = false` unstated. | **Accepted.** Only `login` is gated. `logout`, `logout-all` and `password` stay usable over bearer, with no artificial unavailability. |
| **D4-a** | "Constructors used only by tests" is a convention, not a mechanism, and `#[cfg(test)]` cannot carry it across crate boundaries. | **Accepted.** Non-default `test-harness` cargo feature, self-enabled through dev-dependency configuration, carrying both the relaxed limiter constructor and §10's in-memory constructor. |

**Recorded consequence of D2, known and accepted.** Browsers permit `Secure`
cookies over `http://localhost`, so gating `login` on `api.tls` also removes
session login from a localhost-only HTTP setup. This is collateral of a single
rule, **not a second branch of behaviour** — no host-conditional path is added.
Recorded here so it is not later filed as a bug.

The six decisions the draft left open ([auth-design-draft.md](../../../docs/dashboard/auth-design-draft.md)
§Open implementation decisions) are answered by §2, §3, §4, §5 and §6 below, and
are restated with their reasoning in the review file, as the task requires.

## 1. Shape

Three modules in `fah-api`, one new runtime crate, zero config-schema change,
zero hot-path contact.

```text
crates/fah-api/src/
  auth.rs        middleware — bearer OR cookie, records AuthMethod (exists, extended)
  password.rs    NEW — /config/auth-hash store, Argon2id runner, semaphore, rate limiter
  session.rs     NEW — /data/session-secret, token mint/verify, cookie build/parse
  error.rs       + ApiError::RateLimited, ApiError::Unavailable, per-variant headers (D1)
  routes.rs      four routes inside `v1`, /config guard + doc comment, Origin check
  state.rs       AppState.auth: Arc<AuthState>
  Cargo.toml     + argon2 (new crate), + aws-lc-rs (manifest line only — see §4.1)
                 + [features] test-harness (non-default, D4-a — see §6.1)
```

Layering: everything lands in L3 `fah-api`. `fah-model` and `fah-config` are
untouched, so hard rule 1 and hard rule 2 are unaffected. No `.rs` comment is
added anywhere (hard rule 7); the doc comments named below are the module-level
and item-level `///` forms this crate already uses.

## 2. Storage — two files, no TOML change

| File | Contents | Mode | Rollback |
| ---- | -------- | ---- | -------- |
| `/config/auth-hash` | one PHC line, `$argon2id$v=19$m=19456,t=2,p=1$…` | 0600 | 0.2.20 ignores unknown `/config` files |
| `/data/session-secret` | 32 CSPRNG bytes, hex-encoded | 0600 | same |

`Config` and `fastadhunter.toml` are untouched. This removes B1 (serde-level
redaction destroying the hash through `apply_patch`'s round-trip) and B2
(`deny_unknown_fields` breaking a rollback) outright rather than mitigating them.

Permission restriction follows the existing helper shape at
`crates/fah-api/src/keys.rs:115-116` — unix-only `set_permissions(0o600)`, no
behaviour on other targets.

### 2.1 Staged writes — the actual p5-02 semantics (E3)

Both files use staged-write-then-rename. **These are two independent single
files, not a pair**, so p5-02's `IncompletePair` does not apply to either and is
not cited here. What p5-02 actually does at `crates/fah-api/src/tls.rs:83-110`:

- live file present with a `.tmp` sibling → **`discard` the `.tmp`** (`:91-92`);
- pair member missing with its `.tmp` present → **complete by rename** with a
  `warn!` (`:83-89`);
- `IncompletePair` fires only when one member of the certificate **pair** is
  missing outright — a condition that has no analogue here.

The rules for these two files:

Stray-`.tmp` handling, per file:

| State | Action |
| ----- | ------ |
| `auth-hash` present, `auth-hash.tmp` present | Discard the `.tmp`, `warn!`, boot normally. The rename is atomic, so the live file is already correct. |
| `session-secret` present, `session-secret.tmp` present | Discard the `.tmp`, `warn!`. |
| Either file absent with a `.tmp` sibling | Discard the `.tmp`, never adopt it. An interrupted write is not a credential. The file then counts as absent below. |

The four combined states, all defined, none boot-fatal:

| `auth-hash` | `session-secret` | Meaning | Action |
| ----------- | ---------------- | ------- | ------ |
| present | present | Normal boot | Load both. Nothing is written or rotated. |
| **absent** | present | Out-of-band password reset (§3.1) | Full reset: fresh secret **then** fresh password and hash. |
| **absent** | **absent** | First boot | Same path as the reset — that is the point of §3.1. |
| present | **absent** | **`/data` loss or a deliberate filesystem logout** | **Generate a fresh `session-secret` only. The existing `auth-hash` is preserved and the password does not change.** Every existing session is invalidated, which is the correct fail-safe direction: the secret that authenticated them is genuinely gone. Logged at `warn!` naming the path, so an operator whose `/data` is on ephemeral storage sees why everyone is signed out on each restart. |

The last row is a state **this code never produces**: §3.1 writes the secret
before the hash, so a crash can only ever leave secret-present + hash-absent.
It arrives from outside — a recreated `/data` volume, or an operator deleting
the secret to end every session without touching the password. Defining it as
secret-only regeneration is what keeps that a supported operation instead of an
undefined one.

**No condition in either file is boot-fatal.** An auth artefact must never stop
DNS answering: refusing to boot the household's resolver over a leftover `.tmp`
is a strictly worse outcome than discarding it. A zero-length or
empty-after-trim `auth-hash` counts as **absent** and takes the reset path,
which regenerates rather than leaving the box unauthenticated.

Both stores are loaded in `main.rs` **after** `privilege::drop_to_service_user`
(`crates/fastadhunter/src/main.rs:428`), beside `ApiKeyStore::load_or_create`
(`:430`), so the files are owned by the service user and the permission
restriction is meaningful.

## 3. First boot (decision 6)

`auth-hash` absent → 16 CSPRNG bytes → 26-character base32 password → Argon2id
hash → staged write → logged **once** at `info`, mirroring the API-key line at
`main.rs:431`.

**The plaintext is structurally local (E7).** `AuthState::load_or_create`
returns `(AuthState, Option<String>)`, matching `ApiKeyStore::load_or_create` at
`keys.rs:27`. There is no `generated_password()` accessor and no field on
`AuthState` holding it. The plaintext exists only as a local in `main.rs`,
dropped after the log line. §5.1's guarantee that no handler can reach it is
then a property of the type, not a discipline claim about which methods callers
choose to invoke.

The boot-time hash runs through `spawn_blocking` — one call, roughly 50 ms on
x86 and an estimated ~450 ms on the RB5009 — so it never sits on a runtime
worker while the DNS listeners are already answering.

**Recovery when the log line is gone:** delete `/config/auth-hash`, restart,
read the new line. Documented in SECURITY.md, not implemented as an endpoint —
an endpoint that resets the password without the password is the open
first-visit setup page the task forbids, wearing a different name.

### 3.1 Out-of-band reset rotates the secret too (clarification B)

Deleting `/config/auth-hash` is a **password reset performed by whoever has the
filesystem**, and it must have the same session consequences as the
password-change route. Without this, an operator who resets a forgotten password
leaves every previously issued cookie valid for the remainder of its 7-day
absolute lifetime — including one held by whoever caused the reset to be needed.
That is the exact failure the reset is meant to close.

**Rule: generation and rotation are one atomic step.** `AuthState::load_or_create`
treats "no usable hash" as a reset, not merely as a first boot, and performs both
halves before the listener accepts a request. One algorithm covers all four
states of §2.1's second table — the secret condition is broader than the hash
condition, which is what makes the fourth state secret-only rather than a second
code path:

1. `reset = auth-hash absent or empty-after-trim`;
2. **if `reset` OR `session-secret` absent** → generate a fresh
   `/data/session-secret`, staged-write-then-rename;
3. **if `reset`** → generate the password and its Argon2id hash, then
   staged-write-then-rename `/config/auth-hash`;
4. return the plaintext as the tuple's second element — `Some` only when step 3
   ran — for the single log line.

Step 2's condition is deliberately the weaker one. A missing secret with a live
hash regenerates the secret alone: sessions die, the password survives. A
missing hash always does both, in that order.

Ordering is deliberate. The secret is replaced **before** the hash, so a crash
between the two leaves a box whose old sessions are already dead and whose hash
is still the previous one — locked to the old password, no sessions, recoverable
by repeating the reset. The reverse order would leave the new password live
alongside still-valid old cookies, which is the state this rule exists to make
unreachable.

**This ordering is the single rule; §5.1's password-change route obeys it too
(E1), rather than restating it.** Two paths with opposite orderings was the
defect r2 found.

This makes first boot and reset the same code path, so there is no second path
to keep correct: a first boot rotates a secret that no session has ever used,
which costs 32 random bytes and one file write. The secret-only case is one
extra disjunct on step 2's condition, not a third branch with its own ordering
to get right.

`session-secret` present with `auth-hash` missing is the reset case, handled by
steps 2 and 3. The mirror state — `auth-hash` present with `session-secret`
missing — is handled by step 2 alone, preserving the password. **Neither is an
error, and §2.1 defines no boot-fatal state for either file.**

Boot is single-threaded and precedes the listener, so this path needs no
synchronisation. The runtime paths do — see §5.2.

## 4. Token and cookie (decision 7)

```text
payload = [ver:u8=1][expiry_unix_secs:u64 BE][nonce:16 CSPRNG bytes]   = 25 B
mac     = HMAC-SHA256(secret, payload)                                  = 32 B
token   = hex(payload || mac)                                           = 114 chars
```

- The MAC uses **`aws_lc_rs::hmac`** — see §4.1. No new crate is compiled, and
  `hmac::verify` is constant-time. `argon2` therefore remains the only new
  dependency, as decision 3 requires.
- Hex rather than base64, so no base64 crate is pulled in. `keys.rs` sets the
  precedent.
- The nonce is 128 bits from a CSPRNG, satisfying the API.md requirement.
- No user id and no epoch counter: one account, and revocation is global secret
  rotation. That is the draft's decision 6 answered **no** — `/data` carries no
  revocation state.

**Verification order: length → version → MAC → expiry (E9).** The version byte
is checked, not decorative: an unknown version returns `401` `unauthorized`,
reusing the frozen code. This is a clarity correction only and is not the seed
of a versioning scheme — one accepted value, `1`, and nothing reads it to branch
on format.

Expiry is compared server-side against `SystemTime::now()`; the cookie's
`Max-Age` is a client-side hint a browser may ignore and is never consulted by
the server.

Cookie: `__Host-fah_session=<token>; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age=<lifetime>`.

`__Host-` mandates `Secure`, which is why D2 gates login on `api.tls`; see §5.1.

### 4.1 `aws-lc-rs` reachability — verified, not assumed

Checked before the plan was approved, because "it is already in the tree" and
"it can be called from this crate" are different claims.

| Question | Answer |
| -------- | ------ |
| Is `aws-lc-rs` in the release build of `fah-api`? | **Yes.** `cargo tree -p fah-api -i aws-lc-rs -e normal` shows it under `rcgen v0.14.8` and `rustls v0.23.42`, both **normal** (non-dev) dependencies of `fah-api`. The `reqwest` paths are dev-only and are not what carries it. |
| Which feature turns it on? | `rcgen`'s `aws_lc_rs` feature, selected by `fah-api`'s own default features — so it is on regardless of the dev-dependency graph. |
| Version | `1.17.3`, single copy in `Cargo.lock`. |
| Does the API exist at that version? | **Yes.** `aws_lc_rs::hmac` provides `HMAC_SHA256`, `Key::new`, `sign` and `verify`; the crate also carries a `constant_time` module. Confirmed in the vendored source, not inferred from documentation. |
| Can it be called without declaring it? | **No.** Rust has no transitive-dependency visibility: a crate is nameable only if it appears in that crate's own `Cargo.toml`. |

**Required change:** add `aws-lc-rs = "1.17"` to `crates/fah-api/Cargo.toml`
with default features left on, matching what `rcgen` and `rustls` already
select. Feature unification resolves it to the instance already being compiled,
so this is a manifest line, not a new crate — the same pattern the workspace
already uses for `socket2`. The §11 image measurement is what proves the delta
is zero rather than asserting it.

**Why this and not the fallback (E8).** HMAC-SHA256 is the conventional MAC for
this construction, and `aws_lc_rs::hmac::verify` is constant-time by contract
rather than by inspection. The named fallback — `blake2`'s keyed hashing, which
arrives in the tree with `argon2` anyway — costs the same single manifest line
and stays the escape route.

**Risk accepted, recorded:** this pins `fah-api` directly to `aws-lc-rs`'s
semver, so a future `rustls`/`rcgen` bump to a 2.x line needs a coordinated
bump here instead of arriving silently. If that bump becomes expensive, switch
to the `blake2` fallback rather than carrying the pin.

**Lifetime: 7 days absolute, a compiled-in constant, no config key, no
renewal.** A household phone re-authenticating twice a day is the failure mode
of a short absolute expiry with no sliding window, and decision 7 rules out
sliding. Seven days against `HttpOnly` + `Secure` + `SameSite=Strict` on a
LAN-only origin is the trade being made, and it is stated in the review file so
it can be overridden by changing one constant.

## 5. Routes

All four are registered **inside the `let v1 = Router::new()…;` chain**.
`crates/fah-api/tests/request_coverage.rs:72-75` scrapes that statement
textually and stops at its first `;`, so routes in a second router would be
invisible to the gate — it would pass while the fixtures were missing (B6).

| Route | Auth | Success | Body | `api.tls = false` |
| ----- | ---- | ------- | ---- | ----------------- |
| `POST /api/v1/auth/login` | **exempt** | `204` + `Set-Cookie` | `{"password":"…"}` | **`503` `unavailable`, no `Retry-After`** (D2, D2-a) |
| `POST /api/v1/auth/logout` | required | `204`, cookie cleared | — | unchanged, bearer-reachable (D2-b) |
| `POST /api/v1/auth/logout-all` | required | `204`, cookie cleared, secret rotated | — | unchanged, bearer-reachable, still rotates (D2-b) |
| `POST /api/v1/auth/password` | required | `204`, cookie cleared | `{"current_password","new_password"}` | unchanged, bearer-reachable, current-password check retained (D2-b) |

### 5.0 `Cache-Control: no-store` — exact scope

**Every response from the four auth routes carries it, including failures, and
including the ones the middleware and the error envelope generate.** Spelled
out, because "every auth response" left three paths ambiguous:

| Response | Emitted by | Carries `no-store` |
| -------- | ---------- | ------------------ |
| `204` on all four routes | the handlers | yes |
| `400` `bad_request` — malformed or missing login body | `Json` rejection inside the route | yes |
| `401` `unauthorized` — wrong password | the login handler | yes |
| `401` `unauthorized` — no or bad credential on the other three | `require_auth` middleware, **above** the routes | yes |
| `422` `validation_failed` — short `new_password` | the password handler | yes |
| `429` `rate_limited` | the limiter | yes |
| `503` `unavailable` — saturation and `api.tls = false` | the semaphore and the §5.1 gate | yes |

**Mechanism, reusing what exists.** Two places, because the middleware sits
above the routes and a route-scoped layer cannot reach it:

1. `ApiError::into_response` adds `no-store` to `Unauthorized`, `RateLimited`
   and `Unavailable` through the per-variant header list D1 already introduces.
   No new mechanism.
2. A `SetResponseHeaderLayer` scoped to the four auth routes covers the `204`
   successes and the `400`/`422` envelope paths. `crates/fah-api/src/web.rs:8`
   and `:79` already carry this layer and a `CacheControl` newtype from p5-01 —
   reused, not reinvented.

**Consequence, stated rather than discovered:** path 1 makes **every** `401`,
`429` and `503` across the whole JSON API carry `no-store`, not only those from
the auth routes. That is deliberate — an error envelope is never worth caching,
and path-sniffing inside the middleware to narrow it would be more code for a
worse result. It does not touch the static surface, which sits outside the auth
layer (§5) and keeps p5-01's `no-cache` / `immutable` behaviour unchanged.

**No existing test breaks.** `crates/fah-api/tests/api.rs` asserts status only
on its `401` cases (`:575`, `:602`, `:620`, `:640`, `:663`); the `vary`
assertions at `:642` and `:655` and the empty-`cache-control` assertion at
`web.rs:279` all belong to the static surface. Criterion 8's "existing tests
pass unmodified" therefore holds.

`PUBLIC_PATHS` in `crates/fah-api/src/auth.rs:15` gains `/api/v1/auth/login`
(array width `1` → `2`). This supersedes p5-01's decision-1 sentence that
`PUBLIC_PATHS` stays untouched; the static surface stays merged outside the auth
layer, unchanged.

`/api/v1/auth/login` does not collide with the existing `requests/auth.http`
probe path `/api/v1/auth-probe` — a different segment, and `covers()` matches
segment counts exactly.

**D2-b — only `login` is gated on TLS.** The other three routes are not made
artificially unavailable just because a session cookie cannot be issued.
Scripted password change is legitimate and keeps its current-password
requirement; `logout` with no cookie present is a harmless `204`; `logout-all`
still rotates the session secret, which remains meaningful for any session
issued before `api.tls` was turned off.

### 5.1 Login contract, fixed (clarification A)

```http
POST /api/v1/auth/login
Content-Type: application/json

{"password":"…"}
```

| Outcome | Response |
| ------- | -------- |
| Correct password | `204 No Content`, `Set-Cookie: __Host-fah_session=…`, `Cache-Control: no-store` |
| Wrong password | `401` `unauthorized`, no `Set-Cookie`, `Cache-Control: no-store` |
| Rate limit | `429` `rate_limited` + `Retry-After` |
| Verification saturated | `503` `unavailable` + **`Retry-After: 1`** |
| `api.tls = false` | `503` `unavailable`, **no `Retry-After`**, no `Set-Cookie` |
| Malformed or missing body | `400` `bad_request` |

The two `503` rows share the `unavailable` slug and are told apart by the
header: present means transient, retry; absent means persistent until the
operator changes configuration and restarts.

**No response body on success.** The cookie is the entire result, which is what
keeps the token out of any place a body can be logged, cached or copied into
browser storage. `204` also removes the temptation for a later frontend task to
start reading a body that carries session material.

**The generated first-boot password is never returned by the API.** Not by
login, not by any other route, and not in an error message. Its only channel is
the single `info` log line at generation (§3). E7 makes this structural: the
plaintext never enters `AuthState`, so no handler has anything to read. A test
asserts that the login response body is empty on success and that no response on
any auth route contains the plaintext.

`401` on a wrong password is byte-identical to `401` on a valid password sent
after the hash was replaced: same code, same message, same headers. The failure
message names neither half.

**The `api.tls` check runs first**, before the rate limiter and before the
semaphore, since it spends no Argon2 and must not consume a rate-limit token. A
startup `warn!` states the condition explicitly: session authentication is
unavailable while `api.tls = false`; bearer authentication is unaffected. The
login response remains the authoritative signal for clients, and its message
states that session authentication requires TLS.

**No `Retry-After` on this response (D2-a).** `api.tls` is a boot key
(`crates/fah-config/src/lib.rs:536` for the default, `env.rs:104` for the env
override), so the condition persists until the operator changes configuration
and restarts. Advertising a retry interval would make a conforming client
hot-loop against a condition that never clears. RFC 9110 makes the header
optional on `503`, and its absence is the standard signal for an indefinite
condition — which is what lets a client distinguish this from §6's transient
saturation without a second status or code.

**Password change** verifies the current password through the same
rate limit → semaphore → `spawn_blocking` path as login, enforces a
`new_password` of at least 12 characters (`422`), and then applies §3.1's
ordering (E1):

1. verify the current password (`401` on mismatch, nothing written);
2. **rotate `/data/session-secret`** — staged write → rename → publish;
3. staged-write → rename `/config/auth-hash`;
4. `204`, cookie cleared, **no replacement cookie issued**.

Steps 2 and 3 run inside §5.2's exclusive section. A crash between them leaves
the box on the **old** password with **zero** valid sessions — recoverable by
repeating the change, and never the new-password-with-old-sessions state §3.1
exists to make unreachable. Every session dies, the caller's included, which is
what the acceptance criterion asserts. It emits
`Event::ConfigChanged { restart_required: false }` with the payload unchanged
(decision 5).

### 5.2 Secret rotation — primitive and ordering (E2, E8)

The live secret is held as `tokio::sync::RwLock<SessionSecret>` on `AuthState`.

| Path | Guard | Held across |
| ---- | ----- | ----------- |
| Middleware cookie verification | read | MAC + expiry check only |
| Login | read | Argon2 verification **and** token mint **and** cookie build |
| Password change steps 2–3 | write | secret rotation + hash write |
| `logout-all` | write | secret rotation |
| Boot / out-of-band reset (§3.1) | none | single-threaded, precedes the listener |

**Publication ordering is file first, in-memory second**, so a crash between the
two is recovered by reload-on-boot rather than leaving memory ahead of disk.

**Login holds the read guard across verification and mint.** This is the point
of the guard: without it, a login that verifies the old password just before a
rotation can mint its token just after, producing a session signed with the new
secret that survives the very change meant to kill it. Two concurrent logins
both hold read guards, so `ARGON2_PERMITS` remains the only bound on concurrent
verification and §11.4's measurement is unaffected.

**Accepted cost, recorded.** `tokio::sync::RwLock` is write-preferring, so a
pending rotation blocks new readers, and a rotation may itself wait up to one
Argon2 verification (~50 ms x86, est. ~450 ms RB5009) for an in-flight login to
release. Worst case is a stall of that order on API request authentication.
Admin plane only — nothing on the DNS path takes this lock, so hard rule 3 is
unaffected. The simplest correct design is preferred here over a
rotation-counter retry scheme (engineering principle 12).

**Known, accepted lost update:** two concurrent password changes both verify the
old password before either takes the write guard; both rotate, and the last hash
wins. Not a security hole — both callers knew the old password — and it is
stated rather than designed around.

## 6. Argon2id execution

```text
api.tls check  →  rate limiter (bounded, per-IP)  →  semaphore.try_acquire_owned()  →  spawn_blocking(verify)
 503 unavailable        429 rate_limited                    503 unavailable
 no Retry-After         Retry-After: <window>               Retry-After: 1
 (persistent)           (transient)                         (transient)
```

**`503` semantics, both cases.** One status, one `unavailable` slug, two
causes, told apart by `Retry-After`:

| Cause | `Retry-After` | Clears when |
| ----- | ------------- | ----------- |
| Argon2 verification concurrency saturated | `1` | a permit frees, typically within one verification |
| `api.tls = false` (§5.1) | **absent** | the operator changes configuration and restarts |

- **Parameters**: the OWASP baseline `m = 19456 KiB, t = 2, p = 1` is the
  starting point, confirmed or moved by the §11 measurement — not assumed.
- **Permits**: `ARGON2_PERMITS = 2`, the low end of the task's "start at 1–2 and
  measure before raising".
- **One permit spans a whole operation (E4).** A password change acquires **one**
  permit and holds it across the current-password verification **and** the
  new-password hashing, run **sequentially**, so exactly one 19 MiB arena is
  live per permit at any instant. The transient ceiling is therefore
  `ARGON2_PERMITS × m` = 38 MiB, not `permits × operations × m`.
- **`try_acquire`, never `acquire`.** Queueing would let accepted connections
  park on one permit and consume the 64-slot connection pool
  (`crates/fah-api/src/server.rs:33`), turning a login burst into an API-wide
  outage.
- **Rate limiter**: `Mutex<HashMap<IpAddr, Bucket>>` hard-capped at 128 tracked
  addresses — 5 attempts per 60 s per address, plus a global cap of 30 per 60 s.
  A full map evicts expired entries first; if it is still full the request falls
  back to the global bucket, which fails closed with `429`. Bounded by
  construction, so hard rule 4 holds under a spoofed-source flood. It counts
  **every** attempt rather than only the failures, because the resource being
  protected is Argon2 CPU and RAM, not only guess count. Admin plane only —
  nothing on the DNS path takes this mutex.
- **Client address** from `ConnectInfo<SocketAddr>`, already wired at
  `server.rs:143-148`.
- **`Retry-After`**: seconds until the bucket frees (`429`), or `1` on the
  saturation `503`. The TLS-off `503` carries none (§5.1).

### 6.1 Relaxed limits behind `test-harness` (D4, D4-a)

The limiter's three constants become constructor parameters. **Production
`AuthState` always passes the production values** — 5 attempts / 60 s per source
address, 30 / 60 s global, 128 tracked addresses. There is no config key, no
environment variable, no runtime mutation and no API surface that changes them.

The relaxed constructor exists **only** behind a non-default cargo feature:

```toml
[features]
test-harness = []
```

It carries this constructor and §10's in-memory `AuthState` constructor —
one gate for both, so there is a single thing to audit.

**Why not `#[cfg(test)]`.** `crates/fah-api/tests/api.rs` and
`crates/fastadhunter/tests/history_e2e.rs` link `fah-api` as an ordinary
dependency, so crate-local `cfg(test)` is false in both. This is the same
constraint §10 already hit for the in-memory constructor, and it is why the gate
has to be a feature rather than a `cfg`.

**Enablement, and why it cannot leak into the shipped binary.** The feature is
turned on through dev-dependency configuration — a self dev-dependency in
`crates/fah-api/Cargo.toml` for that crate's own integration tests, and
`features = ["test-harness"]` on `fastadhunter`'s dev-dependency edge for
`history_e2e.rs`. The workspace sets `resolver = "2"`
(`Cargo.toml:2`), so **dev-dependency features are not unified into normal
builds**. The release path is `Dockerfile:86` —
`cargo build --release --locked -p fastadhunter` — with no `--all-features` and
no dev-dependencies in the graph, so the relaxed constructors are absent from
the shipped image by construction, not by convention. Under resolver v1 this
would not hold; the resolver line is load-bearing and must not be changed.

**Never pass `--all-features` to a release build.** The gate at §13 uses it
deliberately and that is test context; the Docker build must not acquire it.

### 6.2 Accepted trade — global-bucket lockout (E11)

When the map is full at 128 addresses, further distinct addresses fall back to
the global bucket, which fails closed. Under a spoofed-source flood the
legitimate operator is locked out of the admin plane until the window rolls.
Memory stays bounded (hard rule 4 holds) and nothing on the DNS path is
affected, so DNS answering continues throughout. Accepted, admin-plane only,
recorded in the review file beside §8's `Host`-derivation assumption.

## 7. Middleware and authentication method (decisions 8, 9)

`require_api_key` becomes `require_auth` in the same module, with the bearer
logic first and unchanged:

1. bearer key matches → insert `AuthMethod::Bearer` into the request extensions;
2. otherwise `__Host-fah_session` present and the token verifies → insert
   `AuthMethod::Session`;
3. otherwise `401` `unauthorized`.

**`?token=` maps to `AuthMethod::Bearer` (E5).** `auth.rs:27` already collapses
both credential sources into one match arm —
`bearer_token(&request).or_else(|| query_token(&request))` — and `query_token`
(`auth.rs:47-60`) is restricted to `/api/v1/events`. Classifying it as `Bearer`
is what keeps §8's `Origin` check off script clients and makes the
"bearer upgrade with no `Origin` still succeeds" criterion provable in both its
header and query forms.

The `Cookie` header is parsed by hand — split on `;`, trim, match the name —
matching the posture of the existing `bearer_token` and `query_token` helpers.
No `axum-extra`, no `cookie` crate.

`ApiError::Unauthorized`'s message changes from `"missing or invalid API key"`
(`error.rs:32`) to wording that covers both halves and reveals neither. Nothing
in the repository asserts the old string. The `code` slug stays `unauthorized`,
which is the frozen contract.

`?token=` on `/api/v1/events` is otherwise untouched. The bearer path is
byte-identical, so the existing `auth.rs` unit tests and the `tests/api.rs`
bearer tests must pass **unmodified** — that is the regression proof, and
editing them would destroy it.

## 8. WebSocket `Origin` validation (decision 8)

In `events_socket` (`crates/fah-api/src/routes.rs:1404`), reading the
`AuthMethod` extension:

- `Bearer` → no check at all; a missing `Origin` is normal for a script, and
  E5 puts `?token=` clients here.
- `Session` → `Origin` must be present and equal the request's own effective
  target origin on scheme + host + port:
  - scheme from `state.config.current().api.tls` — `api.tls` is a boot key, so
    it cannot drift under a live connection;
  - authority from `Host` (HTTP/1.1) or the URI authority (HTTP/2 `:authority`);
  - host compared ASCII-case-insensitively, port normalised against the scheme
    default (443 / 80), IPv6 brackets stripped on both sides.
- Mismatch or missing → `401` `unauthorized`. `403` is not in the code set
  (D1), so no new code is invented here.

**Signature change (E10).** `events_socket` is currently
`async fn events_socket(State<Arc<AppState>>, WebSocketUpgrade) -> Response`.
The check adds:

- the request headers, for `Origin` and `Host`;
- the request URI, for the HTTP/2 `:authority` form;
- `Extension<AuthMethod>`, inserted by §7's middleware.

**The check runs before `state.events.subscribe_socket()`** (`routes.rs:1405`).
p5-03's `SocketSubscription` guard increments the query-subscriber counter when
it is created; rejecting after that call would leak the count and re-arm the
engine-side publish gate on behalf of a socket that never existed. This ordering
is not negotiable and survives the signature change.

**Recorded assumption, carried verbatim into the review file:** deriving the
origin from `Host` is sound only because nothing proxies this listener. Behind a
reverse proxy `Host` is attacker-influenced and this check needs a
forwarded-header policy to mean anything.

There is no configured origin allowlist, for the reason the task gives: it would
break the moment the box is reached by a name other than the configured one,
which is precisely the situation p5-02 maps.

## 9. `/config` (decision 4)

- **`GET /config`** — `auth.*` is not part of `Config` at all, so the API.md
  sentence "redacts **or omits**" is satisfied by omission, structurally. The
  doc comment at `routes.rs:1305-1308` is rewritten to name `auth-hash` and
  `session-secret` alongside the API key and the TLS private key as files that
  live outside this tree, so the invariant it states stays true. Asserted by a
  test against the **response body** — no `auth` key, no `$argon2` substring —
  not by reading the struct.
- **`POST /config`** — an explicit guard on a top-level `auth` key returns `422`
  `validation_failed`, placed beside the `rules.lists` and `policies` guards at
  `routes.rs:1330-1351` and worded the same way. `deny_unknown_fields` would
  already reject it, but the guard is what names the password endpoint in the
  message and what the test pins, so the behaviour survives a future schema
  change.

## 10. Wiring

```rust
let (auth, generated) =
    spawn_blocking(move || fah_api::AuthState::load_or_create(&config_dir, &data_dir)).await??;
if let Some(password) = generated { /* logged once, info; dropped here */ }
```

The tuple return (E7) matches `ApiKeyStore::load_or_create`
(`crates/fah-api/src/keys.rs:27`) and `main.rs:430-434`. `AppStateBuilder` gains
`auth: Arc<AuthState>`.

**`ApiServer::bind`'s signature does not change** — the store is built by the
binary and handed in, exactly as `ApiKeyStore` already is, which keeps the
clocks-and-IO-in-the-binary split (engineering principle 6). Three call sites
are updated, verified as the complete set: `crates/fastadhunter/src/main.rs:456`,
`crates/fah-api/tests/api.rs:497` and
`crates/fastadhunter/tests/history_e2e.rs:435`.

An in-memory constructor taking a known password and a fixed secret serves both
integration harnesses. It cannot be `#[cfg(test)]`-gated, for the reason §6.1
gives, so it sits behind the same non-default `test-harness` feature as the
relaxed limiter constructor (D4-a). No always-available production constructor
is exposed for either.

## 11. Measurement (decision 2, D3, D4)

**Dev leg — inside a Linux container, not on the Windows host.**
`fah_common::process::resident()` reads `/proc`, so a host run would produce no
RSS figure at all. p5-02 established this container procedure.

### 11.1 Dependency, binary and image delta

Build pre-change and post-change images **in the same session**, never against a
stored figure. Record `/fastadhunter` bytes and `docker images` SIZE. Attribute
the delta with `git diff Cargo.lock`, counting **new packages** only — the p5-01
attribution trap, where crates that merely gained an edge were first miscounted
as additions. This is what proves the `aws-lc-rs` manifest line costs zero.

### 11.2 Limiter split (D4)

Two disjoint sets of legs, never mixed:

| Leg | Limiter | Proves |
| --- | ------- | ------ |
| §11.6 burst | **production** constants | the shipped defaults reject a burst with `429` + `Retry-After` |
| §11.3, §11.4, §11.5 | **relaxed**, via the `test-harness` constructor | the semaphore, the transient RSS ceiling, and DNS-unmoved — with the limiter removed as a confounder |

The measurement harness is built with `test-harness` on; the image whose size
and binary bytes §11.1 records is built **without** it, so the delta figures
describe the shipped artefact.

Without the split, every one of these legs measures the limiter instead of the
thing named: at production limits an 8-way login burst from one address yields
2 × `503` and 3 × `429`, and 200 sequential logins take forty minutes.

### 11.3 Login latency

Relaxed limiter. 3 warm-ups, then the median of 21 sequential logins.

### 11.4 Ratchet check, not first-run latency

Relaxed limiter. `GET /api/v1/debug/memory` at baseline → 200 sequential logins
→ re-read → 10 bursts of 8 concurrent logins → re-read → 60 s idle → re-read.
mimalloc failing to return the Argon2 arena would appear as a step that never
comes back down. That is the claim being falsified, and PERFORMANCE.md's ratchet
framing is why a single reading proves nothing.

### 11.5 Concurrency and DNS-unmoved

Relaxed limiter. 8 simultaneous logins → exactly 2 in flight, 6 answered `503`
`unavailable` with `Retry-After: 1`, **zero `429`**, peak RSS delta inside the
stated allowance. DNS query latency sampled during a sustained login burst,
against a no-burst control taken in the same session.

### 11.6 Burst rejection

Production limiter. A 6th attempt from one address inside 60 s answers `429`
`rate_limited` with `Retry-After`.

### 11.7 Allowance and conversion

**Transient allowance, stated separately from the ≤ 128 MB steady-state row:**
≤ 40 MiB above steady-state while verifications are in flight, returning to
baseline within 60 s of idle. Steady-state is 46.6–53.6 MiB today, so the peak
lands near 90 MiB. PERFORMANCE.md §Budgets treats a peak as a transient, and a
criterion written against the steady-state row would not be falsifiable.

**The allowance is conditional on `m = 19456 KiB` (E4).** It is
`ARGON2_PERMITS × m` plus headroom, so if the §11 measurement moves `m`, the
allowance is recomputed as `permits × m + headroom` and **re-measured** before
it counts as a satisfied criterion. A derived number carried across a parameter
change is not evidence.

**Conversion rule.** Latency is multiplied by the measured ~9× x86 → RB5009
factor for the estimate. **RSS is not converted** — memory does not scale with a
CPU factor, and converting it would manufacture a figure
([measurement-traps.md](../../../docs/measurement-traps.md)).

### 11.8 RB5009 leg — owner-run, deferred under D3

The p2.6 L.3 soak is live on tag `soak-p2.6-11` (`1c430aa`, 0.2.20); deploying a
Phase 5 image would end it. This task produces the exact commands and the
recording table; the owner runs the leg after the soak closes. **Nothing in this
task touches the RB5009.**

p5-04 closes as **PASS WITH DEFERRED FINDINGS** with the device leg carried
forward as a live acceptance item — never marked satisfied by conversion. The
carried-forward evidence required is: device and workload; measured login
latency; measured peak RSS and high-water behaviour; DNS-unmoved result.

## 12. Tests

| Acceptance criterion | Test |
| -------------------- | ---- |
| Right and wrong password indistinguishable | `tests/api.rs` — byte-compare: same status, body and headers bar `Set-Cookie` |
| Cookie authenticates `/api/v1/*` | integration, real TLS |
| **Expiry enforced from the token (E6)** | send an **expired** token inside a `Cookie` header while the client-side `Max-Age`/`Expires` is set far in the future; assert `401`. Proves the server never consults the client-side attribute |
| Tampered token rejected | flip one payload byte, MAC verification fails |
| **Unknown token version rejected (E9)** | version byte set to `2`, otherwise well-formed and correctly MAC'd → `401` `unauthorized` |
| Socket upgrades with only the cookie | integration |
| Socket rejects a foreign `Origin`, accepts a matching one | two integration cases |
| **Bearer upgrade with no `Origin` succeeds in both forms (E5)** | `Authorization` header form and `?token=` form |
| Bearer access unchanged | existing tests, **not edited** |
| Password change requires the current password | integration — wrong current password → `401`, nothing written, no rotation |
| Password change kills the old cookie | integration |
| **Password-change crash ordering (E1)** | unit — a crash simulated between secret rotation and hash write leaves the old password valid and zero valid sessions; never new-password-with-old-sessions |
| **No session survives a rotation race (E2)** | unit — a login interleaved with a rotation either completes before it and is invalidated, or is minted after it; no ordering yields a surviving pre-change session |
| `GET /config` carries no auth material | response-body assertion — no `auth` key, no `$argon2` substring |
| `POST /config` with `auth.password_hash` → `422` | integration |
| The session secret never appears in `/config`, logs or errors | unit — `Debug`/`Display` on the secret type are redacted — plus body assertions |
| Argon2id runs on `spawn_blocking` | §11.5 DNS-unmoved measurement. **No unit test is claimed for this**: a unit test on the runner cannot observe whether a runtime worker was free |
| The semaphore bounds concurrency; saturation answers `503` + `Retry-After` | §11.5 with the relaxed limiter, plus an integration case for the status and header |
| Rate limiting is bounded | unit — the map never exceeds 128 entries under 10 000 distinct addresses, production limits |
| Rate limiting rejects a burst | §11.6, production limits — 6th attempt in 60 s → `429` `rate_limited` + `Retry-After` |
| **`api.tls = false` rejects login (D2, D2-a)** | integration — `503` `unavailable`, **`Retry-After` header absent**, no `Set-Cookie`, message naming the TLS requirement |
| **The saturation `503` does carry `Retry-After: 1` (D2-a)** | integration — the same slug with the header present, asserted against the TLS-off case so the two are provably distinguishable |
| **The other three routes stay usable with `api.tls = false` (D2-b)** | integration — bearer `logout` → `204`; bearer `logout-all` → `204` and the secret file changed; bearer `password` with a wrong current password → `401`, with the correct one → `204` |
| **The relaxed limiter is absent without the feature (D4-a)** | compile-time — the `test-harness`-gated constructors do not exist in a default-feature build; asserted by the release image measurement in §11.1 rather than by a runtime test |
| First boot generates, persists the hash only, and reports once | unit on a tempdir, in the shape of the `keys.rs` tests |
| **Login returns `204` with an empty body** and sets the cookie | integration — body length asserted zero, `Set-Cookie` present |
| **No auth response ever carries the generated plaintext** | integration — every auth route's body scanned. E7 makes this structural, so the test is a floor, not the guarantee |
| **Staged-file semantics (E3)** | unit — a `.tmp` beside a live `auth-hash` is discarded and the box boots; a missing `auth-hash` takes the reset path; **no case is boot-fatal** |
| **All four combined store states (§2.1)** | unit, one case each on a tempdir — both present → nothing written, both files byte-identical after boot; hash absent → full reset; both absent → first boot; **hash present + secret absent → the hash is byte-identical afterwards, the secret file is new, a token minted under the old secret no longer verifies, and no plaintext is returned** |
| **`no-store` scope (§5.0)** | integration — assert the header on all seven rows of §5.0's table, including the middleware `401` and both `503` causes |
| **An out-of-band reset kills existing sessions** | unit — mint a token, delete `auth-hash`, re-run `load_or_create`, assert the old token no longer verifies and the secret file changed |
| **Reset ordering survives a crash between the two writes** | unit — a secret rotated with the old hash still in place leaves the box on the old password with no valid sessions, and a repeated reset recovers it |

**`request_coverage.rs` (E12).** `requests/auth.http` gains login, logout,
logout-all and password-change blocks. Fixtures are required because
`routes_from_source()` (`request_coverage.rs:65-90`) enumerates every route
registered in the `v1` chain and `paths_in_request_files()` scrapes
`requests/*.http` for method lines — that is the whole mechanism. **Nothing is
ever issued**, so destructiveness is not a consideration for any of the four and
no `UNCOVERED` entry is needed. The `config/apikey/rotate` precedent is inert for
the same reason.

## 13. Gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
```

No bench: no hot path is touched. `cargo test` includes `request_coverage.rs`,
which is a red gate if a fixture is missing. Nothing in `dashboard/frontend/`
exists yet, so the npm gates do not apply to this task.

`cargo test --all-features` enables `test-harness`, which is the intended
context. `cargo clippy --workspace --all-targets` builds the test targets, so
the dev-dependency edge turns the feature on there too and the gated code is
linted rather than skipped.

The shipped build is unaffected: `Dockerfile:86` is
`cargo build --release --locked -p fastadhunter`, with no `--all-features` and
no dev-dependencies in the graph, and the workspace is on `resolver = "2"`
(`Cargo.toml:2`) so dev-dependency features do not unify into it.

## 14. Documents — proposed, not written (D5)

Approved in scope by D5. Held for the owner to make the concrete edits, per root
CLAUDE.md §Working agreement 1.

| Document | Change |
| -------- | ------ |
| API.md | Promote §Session authentication to live. Add the four route paths, request and response bodies, the token format, the 7-day absolute lifetime, first-run behaviour, and that the generated password is never returned by any route. Expand the code set at `API.md:36` to eight, adding `rate_limited` (429) and `unavailable` (503). **Document `Retry-After` in all three cases (D2-a):** `429 rate_limited` carries it; `503 unavailable` from Argon2 saturation carries `Retry-After: 1`; `503 unavailable` from `api.tls = false` carries none, because `api.tls` is a boot key and the condition persists until restart — the header's presence is the documented discriminator. State that `GET /config` **omits** auth material — the frozen sentence already permits omission. Document that only `login` is gated on TLS and the other three routes stay bearer-usable (D2-b). |
| SECURITY.md | §API access: "No users, roles or sessions in Phase 1" is now false — replace with the session model, the global-revocation trade-off, **the logout vs logout-all distinction** (logout is client-side; the token stays valid until expiry, and `logout-all` is the only revocation), the password-recovery procedure, and §3.1's rule that a reset rotates the session secret. **Also state the mirror recovery (§2.1):** deleting `/data/session-secret` ends every session without changing the password, and losing `/data` has the same effect on the next boot. §Data at rest: `/config/auth-hash` and `/data/session-secret`. The crypto crate list gains `argon2`. Add the `api.tls = false` limitation. |
| CONFIGURATION.md | §First boot gains the generated-password line; §Volumes gains both files. **No `[auth]` section and no mutability class** — decision 1 makes auth a pair of files, not config keys. The phase table's p5-04 row is corrected so it no longer claims a config section or a mutability class. |
| root CLAUDE.md | Hard rule 5's crypto list gains `argon2`, and `aws-lc-rs` if §4.1's direct dependency stands. |
| CONTEXT.md | Untouched by this task — `permitted` belongs to p5-09. |

**Superseded task text, recorded rather than ignored (D5).** The task's sentence
*"CONFIGURATION.md gains the `[auth]` section and its mutability class"* is void
under decision 1 and is corrected in the task file, not silently skipped.

SECURITY.md's `api.tls = false` limitation states both halves: session login is
unavailable, and bearer authentication plus the other three auth routes are
not (D2-b). It also records the localhost collateral from §0.3.

## 15. Out of scope

The login page and anything rendered (p5-05). Certificate UI, CA machinery and
interception (Phase 3). Certificate or SAN work (p5-02). Multiple users, roles,
per-session revocation, TLS client certificates. No new authentication
mechanism. No change to p5-03's `/events` subscription semantics, to
`GET /clients`, or to any other existing contract. No runtime dependency beyond
`argon2` and the `aws-lc-rs` manifest edge of §4.1.

## 16. Order of work

1. `password.rs` and `session.rs` stores, staged writes per §2.1, and the single
   generate-and-rotate path covering all four combined states — including
   secret-only regeneration when the hash survives (§3.1 step 2) — returning
   `(AuthState, Option<String>)`.
2. Token mint and verify with the `length → version → MAC → expiry` order;
   cookie build and parse. `aws-lc-rs` declared in `crates/fah-api/Cargo.toml`.
3. `ApiError::RateLimited` / `ApiError::Unavailable` and the per-variant header
   list (D1), which also carries `no-store` on `Unauthorized`, `RateLimited` and
   `Unavailable` (§5.0). `Retry-After` is set by the call site, not by the
   variant, so the same `Unavailable` can carry it or omit it (D2-a).
4. Middleware: cookie branch, the `AuthMethod` extension, `?token=` → `Bearer`.
5. The four routes, and the §5.0 `no-store` layer over them. The `api.tls` gate
   is on `login` only (D2-b); the other three stay bearer-usable.
6. Argon2 runner, semaphore, rate limiter with production constants, the
   `test-harness` feature and its relaxed constructor (§6.1), and the
   `429` / `503` responses with their `Retry-After` policy (§6).
7. §5.2's `RwLock<SessionSecret>` and the rotation ordering, wired into password
   change and `logout-all`.
8. `/config` guard and the rewritten doc comment.
9. WebSocket `Origin` validation with the §8 extractors, ahead of
   `subscribe_socket()`.
10. Tests, then the `requests/auth.http` fixtures.
11. Dev-leg measurement with the §11.2 limiter split; the RB5009 leg is prepared
    and left for the owner.
12. Propose the documentation edits and wait.

## 17. Reconciled acceptance matrix

Verdict vocabulary: **SATISFIED** — proof exists and is direct once implemented
and green. **CONDITIONAL** — the proof is a measurement that does not exist yet.
**DEFERRED** — carried forward under D3. No criterion is satisfied by the
presence of a test name.

| # | Criterion | Proof | Expected result | Verdict |
| - | --------- | ----- | --------------- | ------- |
| 1 | Correct password sets the cookie; wrong does not; the two are indistinguishable | `tests/api.rs`, byte-compare | Identical status, body, header set; differ only by `Set-Cookie` | SATISFIED once green |
| 2 | A cookie-authenticated request succeeds on `/api/v1/*` | Integration over real TLS | `200` | SATISFIED once green |
| 3 | Expiry enforced from the token | §12 E6 row — expired token in a `Cookie` header, client-side expiry far future | `401`, `unauthorized` envelope | SATISFIED once green |
| 4 | Unknown token version rejected | §12 E9 row | `401` `unauthorized` | SATISFIED once green |
| 5 | Socket upgrades with only the cookie | Integration, matching `Origin` | `101` | SATISFIED once green |
| 6 | Foreign `Origin` rejected, matching accepted | Two integration cases | `401` / `101` | SATISFIED once green |
| 7 | Bearer upgrade with no `Origin` still succeeds | Integration, header **and** `?token=` forms | `101` both | SATISFIED once green |
| 8 | Bearer access unchanged | Existing `auth.rs` unit tests and `tests/api.rs` bearer tests run **unmodified** | Pass with zero diff to those files | SATISFIED once green — the no-edit rule is the proof and must not be relaxed |
| 9 | Password change requires the current password | Integration, wrong current password | `401`, nothing written, no rotation | SATISFIED once green |
| 10 | Password change invalidates every session | Integration (old cookie → `401`) + §12 E1 crash row + §12 E2 race row | Old cookie `401`; crash between rotate and write leaves old password and zero sessions; no interleaving yields a surviving pre-change session | SATISFIED once green — requires **both** E1 and E2 |
| 11 | `GET /config` contains no `auth.*` material | Response-body assertion | No `auth` key, no `$argon2` substring | SATISFIED once green |
| 12 | `POST /config` with `auth.password_hash` → `422` | Integration against the explicit guard | `422` `validation_failed`, message naming the password endpoint | SATISFIED once green |
| 13 | The session secret never appears in `/config`, logs or errors | Unit on redacted `Debug`/`Display` + body assertions | No occurrence anywhere | SATISFIED once green |
| 13b | All four combined store states are defined and none is boot-fatal (§2.1, §3.1) | Unit, one case each on a tempdir | Both present → nothing written; hash absent → full reset; both absent → first boot; **hash present + secret absent → secret regenerated alone, hash byte-identical, old token stops verifying, no plaintext returned** | SATISFIED once green |
| 13c | `no-store` covers every auth-route response, middleware and envelope paths included (§5.0) | Integration across all seven rows of the §5.0 table | Header present on `204`, `400`, both `401` paths, `422`, `429` and both `503` causes | SATISFIED once green |
| 14 | `api.tls = false` login behaviour (D2, D2-a) | Integration | `503` `unavailable`, **no `Retry-After`**, no `Set-Cookie`, message naming the TLS requirement | SATISFIED once green |
| 14b | The two `503` causes are distinguishable | Integration, both cases in one test | Saturation carries `Retry-After: 1`; TLS-off carries no such header; same `unavailable` slug | SATISFIED once green |
| 14c | Only `login` is gated on TLS (D2-b) | Integration, bearer against the other three routes | `logout` `204`; `logout-all` `204` with the secret file changed; `password` `401` then `204` | SATISFIED once green |
| 14d | The relaxed limiter is absent from the shipped build (D4-a) | Compile-time gate + §11.1 image measurement; `resolver = "2"` (`Cargo.toml:2`) and `Dockerfile:86` carrying no `--all-features` | Gated constructors do not exist in a default-feature build | SATISFIED once green — structural, not a runtime assertion |
| 15 | Argon2id runs on `spawn_blocking` | §11.5 DNS-unmoved, relaxed limiter, against a same-session no-burst control | Sampled DNS latency statistically unmoved vs control | **CONDITIONAL** — measurement does not exist yet; no unit test is claimed |
| 16 | Concurrent logins cannot multiply Argon2 memory past the stated allowance; the semaphore bounds them; saturation answers `503` + `Retry-After` | §11.5, relaxed limiter, 8 simultaneous logins | Exactly 2 in flight; 6 × `503` `unavailable` with `Retry-After: 1`; **zero `429`**; peak RSS delta within the allowance | **CONDITIONAL** — measurement does not exist yet |
| 17 | The transient allowance is stated separately from the ≤ 128 MB steady state | §11.4 ratchet, relaxed limiter, `GET /api/v1/debug/memory` at four points | ≤ 40 MiB above steady state at `m = 19456`; back to baseline within 60 s idle; no residual step | **CONDITIONAL** — measurement does not exist yet, and the allowance is void if `m` moves (E4) |
| 18 | Rate limiting is bounded in memory | Unit, 10 000 distinct addresses, production limits | Map never exceeds 128 entries | SATISFIED once green |
| 19 | Rate limiting rejects a burst | §11.6, **production** limits | 6th attempt in 60 s → `429` `rate_limited` + `Retry-After` | SATISFIED once green — must not use the D4 harness |
| 20 | Argon2id parameters recorded with device, workload and measured peak RSS | Dev leg in a Linux container; RB5009 leg owner-run after the soak closes | Device row present with **measured** RSS; latency estimate marked as estimate | **DEFERRED** under D3 — never satisfied by conversion |
| 21 | Gates green, `request_coverage.rs` included | §13 | All three green; `requests/auth.http` carries all four routes | SATISFIED once green |

Totals: 26 criteria — 22 satisfied on implementation, 3 conditional on the `test-harness`
measurement legs and actual measurement (rows 15–17), 1 deferred under D3
(row 20). Row 10 is the only one requiring two corrections together (E1 **and**
E2); row 14d is structural rather than a runtime assertion, and says so.
