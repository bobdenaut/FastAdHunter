# Web Dashboard Authentication — Minimal Design

## Goal

Provide authentication for the household web dashboard without introducing a database or another persistent service solely for one user/password and session state.

## Proposed design

Use:

1. A single **Argon2id password hash** stored in the application's persistent configuration.
2. A separate **random authentication/session secret** stored in persistent `/data`.
3. A compact authenticated session cookie with an expiry.
4. No Postgres session table.

This keeps authentication state to a few hundred bytes plus the existing persistent configuration/`/data` storage.

## Password storage

The user's plaintext password is never stored.

Store only the Argon2id password hash, for example:

```toml
[auth]
password_hash = "$argon2id$v=19$m=19456,t=2,p=1$..."
```

At login:

```text
submitted password
      ↓
Argon2id verification
      ↓
compare with configured password_hash
      ↓
valid → issue authenticated session cookie
```

Argon2id is intended for password storage and should be configured with a memory-hard cost appropriate for the device. OWASP recommends Argon2id and gives a baseline configuration of 19 MiB memory, t=2, p=1. The exact parameters should be benchmarked on the target hardware before being fixed.

## Session authentication

Do not store session IDs in `localStorage` or `sessionStorage`.

Use an HTTP cookie with security attributes such as:

```text
__Host-session=<token>
Secure
HttpOnly
SameSite=Strict
Path=/
```

Use HTTPS for the authenticated web session. The session token must be generated with a cryptographically secure random generator and at least 128 bits of entropy.

The cookie should have an explicit lifetime/expiry appropriate to the dashboard. Prefer short-lived sessions when practical and require a fresh login after expiry.

## Session secret

Keep the session-signing/authentication secret separate from the password hash.

Example persistent location:

```text
/data/auth-secret
```

Properties:

- generated randomly on first initialization;
- never committed to source control;
- protected as secret material;
- persisted across application restarts.

Changing/rotating this secret invalidates existing sessions if stateless signed cookies are used.

## Stateless session model

The server does not need a session database for the normal request path.

Conceptually:

```text
browser
   │
   │  session cookie
   ▼
server
   │
   ├─ verify token authentication/integrity
   ├─ verify expiry
   └─ authorize request
```

The benefit is that there is no per-session database row and no additional container to keep running.

### Important trade-off

A purely stateless cookie cannot revoke one individual session before its expiry unless some server-side revocation state is introduced.

For this household appliance, a simple global invalidation mechanism is sufficient: rotate the persistent authentication/session secret when the password changes or when a global logout is required. That invalidates existing signed sessions without requiring a session table.

If per-session revocation is later required, `/data` can hold bounded revocation state without introducing Postgres.

## Login flow

```text
POST /login
   │
   ├─ validate request
   ├─ verify password against Argon2id hash
   ├─ create authenticated session token
   ├─ Set-Cookie: __Host-session=...
   └─ return success
```

Login failures should not reveal whether a username/password component was the reason for failure. Rate-limit repeated login attempts to reduce online guessing pressure.

## Logout flow

At minimum:

```text
POST /logout
   ↓
expire/delete session cookie
```

For global invalidation:

```text
rotate auth/session secret
   ↓
all existing signed sessions become invalid
```

Responses involved in authentication/session establishment should use restrictive caching such as `Cache-Control: no-store`.

## Configuration / persistence

Suggested separation:

```text
/data/
├── config.toml
│   └── auth.password_hash
└── auth-secret
```

The password hash is not secret in the cryptographic sense; the authentication/session secret is secret material and must be protected accordingly.

File permissions and container mount permissions should prevent unintended access by other local processes/users where applicable.

## Why not Postgres?

For a household box with one account, Postgres adds disproportionate operational and memory overhead for the problem being solved:

```text
one password hash
+ small session state
        ↓
no database is required
```

Avoiding a dedicated database means:

- no second stateful container;
- no database backup/restore lifecycle for auth alone;
- no database upgrade/migration concerns for auth alone;
- less idle RAM consumption on the appliance;
- fewer moving parts and failure modes.

## Security requirements

The implementation should satisfy at least these requirements:

- plaintext passwords are never persisted;
- password verification uses Argon2id with a device-appropriate cost;
- session tokens are generated using a CSPRNG with at least 128 bits of entropy;
- session cookie is `HttpOnly` and `Secure`;
- session cookie uses `SameSite=Strict` unless a documented UX requirement requires `Lax`;
- use the `__Host-` cookie prefix where applicable;
- authenticated traffic is HTTPS-only;
- session expiry is enforced server-side;
- logout invalidates the client cookie;
- password change/global logout rotates the auth/session secret;
- authentication responses are not cached;
- session/authentication tokens are never stored in browser localStorage/sessionStorage;
- login attempts are rate-limited;
- authorization is enforced server-side for every protected endpoint.

## Open implementation decisions

Before implementation, decide and document:

1. Argon2id parameters after measuring login latency and RAM usage on the target device.
2. Session lifetime and whether there is an inactivity timeout in addition to absolute expiry.
3. Exact token format and signing/authentication primitive.
4. Secret generation and first-run initialization behavior.
5. Password-change flow and global session invalidation.
6. Whether `/data` revocation state is needed beyond global secret rotation.

## References

- OWASP Password Storage Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html
- OWASP Session Management Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html
- OWASP Secure Code Review Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/Secure_Code_Review_Cheat_Sheet.html
