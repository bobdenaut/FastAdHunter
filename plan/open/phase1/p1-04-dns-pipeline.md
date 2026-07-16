# P1-04 — DNS Listeners and Pipeline

**Phase:** 1 · **Depends on:** p1-02 · **Model:** Sonnet

## Goal

`fah-dns` accepts queries on UDP/53 + TCP/53 and runs the pipeline front half:
verdict first, blocked-response synthesis.

## Context

ARCHITECTURE.md §DNS Pipeline + ADR-0001 (rules before cache). Wire types come
from `hickory-proto`; listeners bind per CONFIGURATION.md `[dns.listen]`.

## Scope

- UDP listener with EDNS(0) (payload sizes, OPT record echo), TCP listener
  (truncation fallback per RFC; length-prefixed framing).
- Pipeline: decode → Rule Engine verdict → Block ⇒ synthesize `0.0.0.0`/`::`
  (A/AAAA, TTL 10s per config), `$dnsrewrite` payloads honored; Allow/Pass ⇒
  continue to cache/upstream (stubbed until p1-05/06 — temporary passthrough
  forwarder acceptable behind a feature-gate or internal trait).
- Per-worker execution on the Tokio runtime — no central dispatcher, no locks.
- `QueryEvent` emission into a bounded channel (drop-on-full, counter).
- Tests: real DNS packets via `hickory-client` against an ephemeral port —
  blocked domain returns 0.0.0.0/:: with TTL 10, TC-flag path exercises TCP.

## Acceptance criteria

- Blocked queries never touch the network (assert no upstream call).
- Malformed packets dropped without panic (fuzz a corpus of truncated/garbage
  packets).
- Gates green.

## Out of scope

Cache (p1-05), real upstreams (p1-06), DoT/DoH listeners (Phase 3).

## Suggested prompt

> Read ARCHITECTURE.md §DNS Pipeline, ADR-0001, CONFIGURATION.md §[dns.*], and
> plan/wip/phase1/p1-04-dns-pipeline.md. Implement listeners + verdict-first
> pipeline with hickory-proto, QueryEvent emission, and packet-level tests.
