# P1-06 — Upstream Resolvers

**Phase:** 1 · **Depends on:** p1-05 · **Model:** Opus

## Goal

Unanswered queries forward upstream over UDP/TCP, DoT or DoH with ordered
parallel fallback.

## Context

ARCHITECTURE.md §Upstreams + CONFIGURATION.md `[dns.upstreams]`. Hickory +
rustls provide the protocols; defaults are 1.1.1.1 + 9.9.9.9 plain UDP.

## Scope

- Upstream pool from config: per-server protocol (`udp` | `dot` | `doh`),
  connection reuse for DoT/DoH (don't handshake per query).
- Strategy `fallback`: primary first; on timeout (`timeout_ms`) or error, next
  server; UDP truncation → TCP retry to the same server.
- DNSSEC pass-through: DO bit forwarded, RRSIGs returned untouched.
- Failure accounting per upstream (feeds metrics + `/health` degraded state).
- Cache store on success (wires p1-05 into the full pipeline; end of the
  pipeline is now real — remove the p1-04 temporary forwarder).
- Tests: against a local mock DNS server (hickory server on ephemeral port):
  fallback on timeout, TCP retry on truncation, DO-bit passthrough; DoT/DoH
  smoke tests against a containerized or public resolver marked `#[ignore]`
  for offline runs.

## Acceptance criteria

- Primary down ⇒ answers still arrive within one extra timeout window.
- No per-query TLS handshakes (connection reuse asserted via counters).
- Gates green.

## Out of scope

Load-balancing strategies (backlog), DNSSEC validation (backlog).

## Suggested prompt

> Read ARCHITECTURE.md §Upstreams, CONFIGURATION.md §[dns.upstreams], and
> plan/wip/phase1/p1-06-upstreams.md. Implement the pool with fallback
> strategy, DoT/DoH via hickory+rustls, and mock-server tests.
