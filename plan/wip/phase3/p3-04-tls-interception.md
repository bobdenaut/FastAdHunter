# P3-04 — TLS Interception

**Phase:** 3 · **Depends on:** p3-01, p3-03 · **Model:** Fable

## Goal

Managed clients get full URL-level filtering inside HTTPS: terminate TLS with
minted certs, run the Phase 2 HTTP pipeline, re-encrypt upstream.

## Context

SECURITY.md principles are hard law: opt-in, per-client, never default. The
SNI path (p3-03) already handled the connection — interception is a per-client
branch taken instead of splicing. Decrypted request handling reuses the
Phase 2 filtering pipeline verbatim (fah-http does not grow a second pipeline).

## Scope

- Interception decision: client must be explicitly listed
  (`[https.interception] clients = [...]` or per-policy flag —
  CONFIGURATION.md updated; default empty). Identity is IP/CIDR — the only
  identity the container sees — so GAR §5.14 (stable identity, not bare IPs)
  needs the owner decision the plan spells out: static DHCP lease as a
  documented precondition, or defer interception.
- Exclusions that always splice even for intercepted clients: shipped
  baseline list of known-pinned domains (banking hints, OS update hosts) +
  user-extendable; SNI matched before terminating.
- Downstream: rustls server config with p3-01 minted leaf for the SNI;
  upstream: rustls client with proper verification (system/webpki roots;
  upstream cert failures ⇒ close, surfaced in events — we never present a
  valid cert for an upstream we couldn't verify).
- HTTP/1.1 and HTTP/2 through hyper into the Phase 2 pipeline (RequestEvent
  `kind: https`); streaming preserved; ALPN negotiated both sides.
- HSTS is transparent (we present a trusted-by-client CA cert); document why
  in SECURITY.md §interception.
- Tests: intercepted client with CA trusted — ad URL inside HTTPS blocked,
  page loads; excluded domain splices (observed as passthrough); upstream
  with bad cert refused; non-listed client always splices.

## Acceptance criteria

- A client NOT in the interception list can never be intercepted (test
  proves the branch, config fuzz keeps it false).
- Upstream verification failures never produce a locally-signed success
  (test with self-signed upstream).
- SECURITY.md + CONFIGURATION.md updated in the same change.
- Gates green.

## Out of scope

HTML rewriting (Phase 4 — flows through this pipe when it lands), QUIC.

## Suggested prompt

> Read SECURITY.md (all interception principles), plan/wip/phase3/
> p3-04-tls-interception.md, and the p2-04 pipeline. Implement the per-client
> interception branch with exclusions, strict upstream verification and ALPN,
> feeding the existing HTTP pipeline; update the two docs; prove every
> security property with tests.
