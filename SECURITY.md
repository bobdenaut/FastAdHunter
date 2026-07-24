# SECURITY

Security model of FastAdHunter: API access, TLS, certificates, container
hardening. Guiding rule: **do not reinvent cryptography** — rustls, rcgen,
x509-parser only; no hand-rolled TLS or crypto anywhere.

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
- Required for everything under `/api/v1/`. `GET /health` and `GET /metrics`
  are exempt by default (configurable — `api.metrics_public`).
- Rotation: `POST /api/v1/config/apikey/rotate` — new key returned once, old
  key invalid immediately.
- No users, roles or sessions in Phase 1: single-admin appliance.

## TLS for the API

- **HTTPS is the default from Phase 1.** On first boot a self-signed
  certificate is generated with rcgen and stored in `/config`.
- The browser shows a one-time warning for the self-signed certificate —
  expected for an appliance; the certificate is stable across restarts.
- Users can replace it with their own certificate (PEM/PFX) in `/config`
  (Phase 3 adds API-driven import).
- Plain HTTP requires an explicit opt-out (`api.tls = false`) and is
  documented as **unsafe**: the API key is a bearer token — over HTTP a single
  sniffed request leaks full admin control and the DNS history.

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

- `/config` holds secrets (API key, TLS private key) — back it up accordingly;
  file permissions restricted to the container user.
- `/data` (query log, cached lists, snapshots, history rollups + perf series)
  holds the DNS history — treat the SSD as sensitive when disposing of it.
  Retention is bounded and configurable; disabling the query log
  (`query_log.enabled = false`) is the privacy-maximal setting.
- `[history]` widens the retained-data window: it keeps hourly/daily aggregates
  and per-client top-N for `history.retention_days` (default 30, up to 90),
  longer than the query log's raw segments. It stores aggregates, not raw
  per-query rows, but `enabled = false` turns it off entirely — mirroring
  `query_log.enabled` — for the privacy-maximal posture.

## Later phases (principles fixed now)

- **Phase 3 — HTTPS interception (MITM)** is opt-in, per-managed-environment,
  never default. The generated CA's private key never leaves `/config`; CA
  export endpoints export the **public** certificate only. Interception uses
  rustls; certificate minting uses rcgen; parsing uses x509-parser.
- DoT/DoH **listeners** (client-facing) arrive with Phase 3 certificate
  machinery so clients can actually validate what they connect to.

## Reporting

Vulnerabilities: open a private security advisory (GitHub) or contact the
maintainer directly. Do not open public issues for exploitable bugs.
