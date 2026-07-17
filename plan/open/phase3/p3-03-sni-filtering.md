# P3-03 — SNI Filtering

**Phase:** 3 · **Depends on:** phase2 · **Model:** Sonnet

## Goal

HTTPS traffic gets domain-level filtering without decryption: read the SNI,
ask the Rule Engine, block or splice.

## Context

Port 443 dst-nats to the container (like port 80 in Phase 2). We parse only
the TLS ClientHello — never terminate TLS here. Verdict uses the existing
domain matcher (same one DNS uses), so every client benefits with zero setup.
This is the default HTTPS path; interception (p3-04) is the opt-in exception.

## Scope

- TLS ClientHello parser: extract SNI without a TLS stack (frame parse only —
  bounded read, no allocation per connection beyond the buffer; reject
  oversized/fragmented-forever hellos).
- Verdict on SNI hostname via the client's policy (Phase 2 resolution).
- Block ⇒ close the TCP connection immediately (browsers show a network
  error; cheap and unambiguous). Pass ⇒ splice bytes both directions
  (tokio copy_bidirectional), bounded buffers, no inspection past the hello.
- No SNI / ECH-encrypted SNI ⇒ configurable: `pass` (default) or `block`
  (CONFIGURATION.md `[https.sni]` — doc updated same change; note the ECH
  limitation in SECURITY.md).
- Events: RequestEvent with `kind: https-sni`, host, verdict (query-log +
  metrics dimensions extended; API.md filter values updated).
- Operating mode: active in `dns+http+https` only.
- Tests: real rustls client against the splice path (handshake completes
  end-to-end through us), blocked SNI → connection refused before any
  upstream contact, garbage-on-443 fuzz, throughput bench (splice overhead).

## Acceptance criteria

- Blocked domain: zero bytes to upstream (asserted).
- Spliced HTTPS session byte-identical and budget-fast (bench recorded).
- Docs updated: CONFIGURATION.md, SECURITY.md (ECH note), API.md kinds.
- Gates green.

## Out of scope

Decryption/MITM (p3-04), QUIC/HTTP3 (documented limitation, backlog).

## Suggested prompt

> Read plan/wip/phase3/p3-03-sni-filtering.md, PERFORMANCE.md golden rules,
> and the Phase 2 policy resolution. Implement the ClientHello SNI parse,
> verdict, close-or-splice paths with events, doc updates, tests and bench.
