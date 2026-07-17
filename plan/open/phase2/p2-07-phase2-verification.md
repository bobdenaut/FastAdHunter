# P2-07 — Phase 2 Verification

**Phase:** 2 · **Depends on:** p2-06 · **Model:** Sonnet

## Goal

Phase 2 proven: HTTP budgets defined and met, end-to-end tests cover the new
surface, dns+http validated on the RB5009.

## Context

PERFORMANCE.md has no HTTP numbers yet — this task sets them (doc update),
then proves them. On-device steps need the user (RouterOS dst-nat rule for
port 80 → container).

## Scope

- PERFORMANCE.md: add HTTP budget rows — pass-through added latency p99,
  request-verdict latency, proxied throughput (target order: saturate 1 Gbps
  LAN on large bodies), RAM ceiling unchanged (≤128MB steady with EasyList +
  policies loaded). Justify numbers from p2-02/03 bench data.
- Bench consolidation: HTTP benches map 1:1 to the new budget rows.
- End-to-end (offline): mock origin + real binary in dns+http mode —
  page with ad script: script blocked (200-empty), page renders; second
  client under stricter policy: page domain itself blocked; WS stream shows
  both kinds of events.
- RB5009 (with the user): document + apply dst-nat rule, browse plain-HTTP
  site through it, verify filtering + pass-through speed, extend
  docs/deploy-rb5009.md with the HTTP section + rollback (drop the nat rule).
- Soak: 24h dns+http; RAM/latency/QPS recorded vs updated budgets.

## Acceptance criteria

- Updated PERFORMANCE.md budget table fully bench-backed; dev numbers meet it.
- On-device: no regression on DNS soak numbers; HTTP pass-through
  imperceptible in normal browsing (user confirms); numbers recorded.
- Gates green.

## Out of scope

HTTPS (Phase 3), HTML rewriting (Phase 4).

## Suggested prompt

> Read plan/wip/phase2/p2-07-phase2-verification.md and PERFORMANCE.md.
> Set the HTTP budget rows from bench data, consolidate benches, write the
> offline e2e scenarios, then walk the RB5009 dst-nat setup and soak WITH the
> user and record results.
