# SECURITY

Security model of FastAdHunter: API access, TLS, certificates, container
hardening. Guiding rule: **do not reinvent cryptography** — rustls, rcgen,
x509-parser, argon2 and aws-lc-rs only; no hand-rolled TLS or crypto anywhere.
`argon2` hashes the dashboard password; `aws-lc-rs` supplies the constant-time
HMAC-SHA256 the session token is signed and verified with (it is already in the
tree through rustls and rcgen). The set is closed to *cryptography*; `pem`
(base64 framing, no crypto) is used to re-encode certificates for export.

## Threat model (summary)

- The engine runs on the home router — it sees the DNS history of every device.
  That history is sensitive personal data.
- The LAN is **not** trusted: any compromised IoT device can sniff traffic or
  ARP-spoof. Plaintext admin traffic on the LAN is an account takeover waiting
  to happen.
- The WAN is hostile. Nothing administrative is ever exposed there.

## API access

- **Single API key** (bearer token), generated on first boot:
  printed once to the container log and stored in `/config`.
- Required for everything under `/api/v1/`. `GET /health` and
  `POST /api/v1/auth/login` are the two exemptions inside the admin surface
  and are not configurable; `/health` returns status, version and uptime
  only. `/dns-query` (DoH, API.md §DNS over HTTPS) is outside `/api/v1/` and
  unauthenticated because a DNS client can present neither a key nor a
  cookie; it answers only DNS and reaches no admin handler.
- Rotation: `POST /api/v1/config/apikey/rotate` — new key returned once, old
  key invalid immediately.
- **No users and no roles: single-admin appliance.** One password, one API key.
  Sessions exist from Phase 5 (below) but carry no identity — a session is
  "someone who knew the password", nothing more.

### Dashboard sessions (Phase 5)

The browser signs in with a password and rides a signed cookie. Scripts keep
using the bearer key, unchanged.

| Artefact | Where | Contents |
| -------- | ----- | -------- |
| Password | `/config/auth-hash`, mode 0600 | one Argon2id PHC line, `m = 19456 KiB, t = 2, p = 1` |
| Session secret | `/data/session-secret`, mode 0600 | 32 CSPRNG bytes, hex |

The cookie is `__Host-fah_session`, `Secure`, `HttpOnly`, `SameSite=Strict`,
`Path=/`. The token is `hex(payload ‖ HMAC-SHA256)` over a version byte, an
absolute expiry and a 128-bit nonce. **The expiry inside the token is
authoritative and is checked server-side**; the cookie attribute is a
client-side hint the server never consults.

**Lifetime is 7 days, absolute — no sliding renewal, no inactivity timeout.**
The trade is deliberate and is the one to revisit first: re-authenticating a
household phone twice a day is the failure mode of a short absolute expiry, and
it is bought with `HttpOnly` + `Secure` + `SameSite=Strict` on a LAN-only
origin. One compiled-in constant changes it.

**Revocation is global, and that is the whole model.** There is no session
table and no per-session revocation:

| Action | Effect |
| ------ | ------ |
| `POST /api/v1/auth/logout` | **Client-side only.** Clears the cookie; the token stays valid until its expiry. |
| `POST /api/v1/auth/logout-all` | **The only revocation.** Rotates `/data/session-secret` — every session everywhere ends at once, including the caller's. |
| `POST /api/v1/auth/password` | Requires the current password, then rotates the secret and replaces the hash. Every session ends. |

Password change requires the current password. With `SameSite=Strict` and no
CSRF token, that check is the remaining barrier for an unattended logged-in
browser, and it is the reauthentication a privileged operation should carry
anyway.

**Online guessing is bounded** — 5 attempts per source address per 60 s, 30 per
60 s globally, at most 128 tracked addresses. Verification runs off the async
runtime behind a 2-permit semaphore; saturation answers `503` with
`Retry-After: 1` rather than queueing. A spoofed-source flood can exhaust the
tracked-address map and lock the operator out of the **admin plane** until the
window rolls; DNS answering is unaffected, and memory stays bounded. Accepted.

**WebSocket upgrades are origin-checked.** A cookie-authenticated upgrade must
carry an `Origin` matching the request's own effective origin; a
bearer-authenticated one need not, and its absence is normal for scripts.
Browsers send cookies on a handshake and a WebSocket has no same-origin policy,
so `SameSite` is defence in depth here rather than the whole defence.

**The origin is derived from `Host`, which is sound only because nothing proxies
this listener.** Behind a reverse proxy `Host` becomes attacker-influenced and
this check needs a forwarded-header policy before it means anything.

### Password recovery

There is **no** password-reset endpoint. An endpoint that resets the password
without the password is an open first-visit setup page wearing another name —
on a LAN, whoever reaches it first owns the box.

Recovery is filesystem-side:

1. Delete `/config/auth-hash`.
2. Restart. The next boot generates a new password and prints it **once** to
   the container log, exactly as the API key line does.

**A reset also rotates `/data/session-secret`**, before the new hash is written,
so no session issued before the reset survives it — including one held by
whoever caused the reset to be needed. A crash between the two writes leaves the
old password with zero valid sessions, which is recoverable by repeating the
reset.

The mirror operation: **deleting `/data/session-secret` alone ends every session
without changing the password.** Losing `/data` has the same effect on the next
boot, logged at `warn!` — an operator whose `/data` is on ephemeral storage sees
why everyone is signed out on each restart.

### `api.tls = false` removes session login

Session authentication requires TLS: the `__Host-` cookie prefix mandates
`Secure`. With `api.tls = false`, `POST /api/v1/auth/login` answers `503`
`unavailable` with **no** `Retry-After` — `api.tls` is a boot key, so the
condition persists until the operator changes configuration and restarts.

Unaffected: bearer-key authentication, and the other three `/api/v1/auth`
routes. `logout-all` still rotates the secret; `password` still requires the
current password.

Known collateral, recorded so it is not later filed as a bug: browsers permit
`Secure` cookies over `http://localhost`, so this single rule also removes
session login from a localhost-only HTTP setup. There is no host-conditional
branch.

## TLS for the API

- **HTTPS is the default from Phase 1.** On first boot a self-signed
  certificate is generated with rcgen and stored in `/config`.
- The browser shows a one-time warning for the self-signed certificate —
  expected for an appliance; the certificate is stable across restarts.
- Users can replace it with their own certificate (**PEM**) in `/config`, or
  through `POST /api/v1/certificates/import` (API.md §Certificates) — the pair
  is validated, the one it replaces is archived, and it takes effect at the
  next restart. PKCS#12/PFX is **not** accepted: the fixed
  crypto set above has no PKCS#12 parser and real `.pfx` files are encrypted, so
  supporting them would mean adding several crypto crates. Convert first with
  `openssl pkcs12 -in cert.pfx -out cert.pem -nodes`
  ([ADR-0006](docs/decisions/0006-certificate-machinery-home.md)).
- An imported certificate is re-encoded from the parsed DER before it is stored,
  so private key material pasted into the certificate field is discarded rather
  than written to a world-readable file or handed back by an export endpoint.
- Plain HTTP requires an explicit opt-out (`api.tls = false`) and is
  documented as **unsafe**: the API key is a bearer token — over HTTP a single
  sniffed request leaks full admin control and the DNS history.

### What a browser actually shows

The certificate is self-signed, so every browser warns on the first visit.
Chrome reports `ERR_CERT_AUTHORITY_INVALID` — "Your connection is not private"
— and proceeding takes two actions: **Advanced**, then **Proceed to
\<address\> (unsafe)**. The exception, and any cookie set afterwards, survive a
browser restart. *(Measured: Chrome 151.0.7922.174, Windows 11, fresh profile,
IP-literal origin.)*

The certificate covers the box's own LAN address, discovered at generation
time, alongside `fastadhunter`, `localhost`, `127.0.0.1` and `::1`. It is valid
for 397 days from first boot and does **not** renew itself; regenerating it is
an operator action and invalidates every accepted browser exception once.

**Phones work.** Measured on Android 16 with Brave 1.93.138 against an
IP-literal origin: after the one-time warning is accepted, the browser treats
the origin as secure, and a `Secure`, `__Host-`-prefixed session cookie
survives both a full browser restart and a device reboot. The address bar keeps
a "not secure" marker; it does not affect the session. iOS Safari has not been
measured.

### What a household should do

| Option | Cost | What it gets |
| ------ | ---- | ------------ |
| Accept the warning once, per device and per browser | 2 actions per device; repeated after any certificate regeneration | A working dashboard. The address bar keeps saying "Not secure" |
| Install the box certificate as a trusted root on each device | One install per device, plus a repeat after each regeneration | A clean connection with no warning — verified: `openssl` returns `0 (ok)` once the certificate is trusted **and** the address is in its SAN set. Trusting a certificate whose SAN set misses the address still fails with `64 (IP address mismatch)`, which is why the SAN set matters more than the warning does |
| A real name with a publicly trusted certificate | A domain, DNS, and a renewal mechanism this project does not ship | No warning anywhere, no per-device work. Out of scope until Phase 3 |

`fastadhunter` is in the certificate but resolves nowhere: reaching the
dashboard by name needs a DNS entry or a hosts file; without one the browser
fails with `ERR_NAME_NOT_RESOLVED` before TLS is ever attempted.

Do not use `https://localhost:8443/` to judge whether the certificate works —
browsers treat `localhost` as a secure origin regardless of TLS, so it hides
exactly the failure worth finding.

## Network exposure

- Bind addresses are configurable; the API must face the LAN side only.
- **Never expose port 8443 (or 53) toward the WAN.** No port-forwarding, no
  DMZ. Remote access belongs behind a VPN (e.g. WireGuard on the router).
- DNS (53/udp+tcp) serves LAN clients; an open resolver on the WAN is a DDoS
  amplification liability.

## Upstream privacy

- Encrypted upstreams (DoT/DoH) supported from Phase 1, opt-in
  (see [CONFIGURATION.md](CONFIGURATION.md)) — without them the ISP sees every
  DNS query in plaintext.

## Container hardening

- **Distroless/static image**: no shell, no package manager, no interpreter —
  the attack surface is one static binary plus a CA bundle.
- **Serves as non-root.** The entrypoint starts as root solely to bind port 53,
  then drops permanently to uid/gid 65532 — supplementary groups cleared,
  `setgid` before `setuid`, and the drop verified irreversible — before any
  query is answered or the API is bound. No query, no HTTP request and no
  write to `/config` or `/data` is ever handled by a privileged process.
  Started unprivileged (a high port, or a runtime that permits 53), it stays
  that way and the drop is a no-op. See
  [ADR-0004](docs/decisions/0004-privileged-port-binding.md) for why file
  capabilities are not used instead — RouterOS strips them on import.
- Root filesystem read-only; writes only to the `/config` and `/data` mounts.
- Healthcheck via `fastadhunter --healthcheck` (self-probe; no shell tools).

## Data at rest

- `/config` holds secrets (API key, TLS private key, `auth-hash`) — back it up
  accordingly; file permissions restricted to the container user.
- `/config/ca-archive/` and `/config/api-archive/` retain **every** superseded
  private key (CA regeneration, API-pair import), mode 0600, at most 8 each —
  the cap refuses further replacements rather than pruning. A backup, a copy
  of `/config`, or an SSD disposal covers those keys too; a retired CA key can
  still sign leaves any device that trusted that root will accept.
- `/config/auth-hash` is the Argon2id hash of the dashboard password. It is a
  hash, not a licence to publish it: it is offline-crackable material, so it
  never enters a response body and `GET /api/v1/config` omits it entirely.
- `/data/session-secret` is the HMAC key every live session is signed with.
  Never logged, never returned, redacted in `Debug` and `Display`. Treat it as
  equivalent to every currently signed-in browser.
- `/data` (cached lists, snapshots, history rollups + perf series) holds the DNS
  history — treat the SSD as sensitive when disposing of it. Retention is
  bounded and configurable.
- **No per-query row is written to disk.** `/data` holds aggregates only; the
  live per-query feed is `WS /api/v1/events`, which persists nothing.
- `[history]` sets the retained-data window: hourly/daily aggregates and
  per-client top-N for `history.retention_days` (default 30, up to 90). It
  stores aggregates, not raw per-query rows, and `enabled = false` turns it off
  entirely — the privacy-maximal posture.

## Later phases (principles fixed now)

- **Phase 3 — SNI filtering (p3-03)** decrypts nothing. The ClientHello is
  parsed as bytes (strict bounds on every length; a malformed or non-TLS
  connection is closed and counted, never forwarded), the SNI hostname is
  judged by the same Rule Engine and per-client policy the DNS path uses, and
  the connection is then either closed or relayed **uninspected** in both
  directions. No key material, no CA and no `/config` access exist on this
  path. The SNI hostname is attacker-controlled, so it is handed to the
  resolver and then to the **egress guard** (`fah_common::egress`), which
  judges the *resolved* address — the same open-relay defence the `Host` header
  gets on :80.
- **ECH / no-SNI is a hard transport limit, not a policy choice.** A TLS
  connection reaching the dst-nat'd :443 with no plaintext SNI has **no
  recoverable destination**: the container's netns holds no conntrack record of
  the router-side NAT, so `getsockopt(SO_ORIGINAL_DST)` returns `ENOENT`
  (measured on-device 2026-08-31, `docs/routeros-traps.md`). Such a connection
  is closed; `[https.sni] no_sni` only decides whether it is *reported* as pass
  or block. The DNS layer remains the backstop for domains hidden behind ECH.
- **Domain fronting is an inherent SNI-filter bypass.** The SNI judge sees
  only the name the client *claims*; a client may send an allowed SNI, be
  spliced to that name's address, and then ask for a blocked host in the
  encrypted `Host` header / `:authority` of a CDN that serves both. Nothing
  on this path can see or refuse that. Two backstops: the DNS layer, which
  still refuses the blocked name if the client resolves it honestly, and
  p3-04 interception for managed clients, which judges the real request.
  Major CDNs reject mismatched SNI/Host today, but that is their policy, not
  ours.
- **Phase 3 — HTTPS interception (MITM, p3-04)** is opt-in, per-client,
  never default: only a client whose address is listed in
  `[https.interception] clients` (IP/CIDR — the only identity the container
  sees, so every listed client must hold a static lease, CONFIGURATION.md)
  takes the terminate leg; everyone else, and every excluded SNI, splices as
  in p3-03. The generated CA's private key never leaves `/config`; the
  interception path only reads minted leaves from the p3-01 cache. CA export
  endpoints export the **public** certificate only, re-encoded from the parsed
  certificate DER so no export path can reach a key. Interception uses rustls;
  certificate minting uses rcgen; parsing uses x509-parser. All of it lives in
  `fah-certs` ([ADR-0006](docs/decisions/0006-certificate-machinery-home.md)).
  - **Verify before present.** The upstream is connected to the
    egress-approved address and its certificate verified against the SNI
    hostname (compiled-in webpki roots — distroless has no system store)
    **before** our ServerHello is sent. A verification failure closes the
    client's TCP connection unanswered: the client never sees a locally-signed
    certificate for an origin we could not verify. Only
    `rustls::Error::InvalidCertificate` is a verification failure (event
    `status 526`); every other upstream failure — refused, timed out, ALPN or
    protocol mismatch — is an ordinary upstream failure (`status 0`). A
    reconnect inside a session repeats the same verification, or resumes the
    TLS session the origin issued a ticket for, which is bound to that
    verification.
  - **One name per session.** A request whose `Host`/`:authority` is not the
    verified SNI is refused with `421`; nothing rides a session verified for
    another name.
  - **HSTS is transparent.** The minted leaf chains to the CA the client
    installed, so HSTS pins hold and no browser warning is bypassed —
    interception works only where the CA was deliberately trusted.
  - **Exclusions always splice.** A compiled-in baseline of certificate-pinned
    families (OS update and push hosts, app stores, messengers, payment and
    banking apps — `fah_http::BASELINE_EXCLUSIONS`) plus
    `[https.interception] exclude_domains` is matched on the SNI, before any
    decryption; a hit takes the p3-03 splice leg.
  - **Listed clients lose ECH.** An Encrypted ClientHello enters the terminate
    leg under its outer (public) SNI, is verified and served under that name,
    and the browser retries without ECH; the retry is filtered under the real
    name. Inherent to MITM; unlisted clients are unaffected.
  - **Bounded per session.** An intercepted session holds one client and one
    upstream TLS session plus fixed hyper buffers (CONFIGURATION.md
    `[https] max_connections`); it never buffers a body.
- DoT (`[dns.listen] dot_port`, 853) and DoH (`/dns-query`) **listeners**
  (client-facing) ship with the Phase 3 certificate machinery. DoT presents a
  CA-minted leaf for the SNI the client sends when a CA exists, else the API
  pair (the imported-real-certificate route); a hello without SNI gets the
  API pair. Never plaintext on 853 — a non-TLS client gets a TLS alert or a
  close. DoH rides the API listener's certificate.

## Reporting

Vulnerabilities: open a private security advisory (GitHub) or contact the
maintainer directly. Do not open public issues for exploitable bugs.
