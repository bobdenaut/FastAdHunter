# P3-05 — DoT and DoH Listeners

**Phase:** 3 · **Depends on:** p3-01 · **Model:** Opus

## Goal

Clients speak encrypted DNS **to us**: DoT on :853, DoH under the API server;
Android Private DNS works against the container.

## Context

Phase 1 deferred client-facing encrypted DNS until the cert story existed —
it now does (p3-01). The DNS pipeline is untouched: these are new front doors
into the same `fah-dns` pipeline entry (decode differs, everything after is
shared).

## Scope

- DoT listener (tokio-rustls, port 853, RFC 7858): TLS with the API server
  certificate (imported or self-signed; document that Android Private DNS
  requires a cert the device trusts — hostname-validated, so a real
  hostname + imported cert, or the CA route); 2-byte length framing;
  connection reuse; idle timeouts; bounded concurrent connections.
- DoH endpoint (RFC 8484): `POST/GET /dns-query` (wireformat) served by the
  existing axum server (fah-api hosts the route, handler delegates to a
  fah-dns handle — layering respected: both are L3, wiring via the binary's
  shared handle, no sibling import).
- Config: `[dns.listen] dot_enabled/dot_port`, `doh_enabled`
  (CONFIGURATION.md updated); client IP for policy resolution = TLS peer
  address.
- Query log/metrics: transport dimension (`udp|tcp|dot|doh`) on QueryEvent
  (API.md filter updated).
- Tests: hickory client over DoT and DoH against ephemeral listeners —
  blocked/allowed verdicts identical to UDP; policy resolution uses the
  right client IP; concurrent-connection bound enforced.

## Acceptance criteria

- Same domain, same client, same verdict across UDP/DoT/DoH (test matrix).
- Android Private DNS setup documented (p3-06 walks it with the user).
- CONFIGURATION.md + API.md updated in the same change.
- Gates green.

## Out of scope

DoH over HTTP/3 (backlog), DNS-over-QUIC (backlog).

## Suggested prompt

> Read plan/wip/phase3/p3-05-dot-doh-listeners.md, RFC 7858/8484 framing
> essentials, ARCHITECTURE.md wiring rules, and CONFIGURATION.md. Add the DoT
> listener and DoH route delegating into the existing pipeline, with the
> transport dimension and the UDP/DoT/DoH verdict-parity test matrix.
