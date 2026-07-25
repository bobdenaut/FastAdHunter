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
  request-verdict latency, proxied throughput, RAM ceiling. **Derive every row
  from measured p2-02/p2-03 data; do not carry a target in from ambition.**
- **Throughput is two rows, not one, because the body is never parsed.**
  Images, ZIPs, PDFs, video, fonts, any non-HTML body: the verdict is taken on
  the *head*, then the bytes are streamed through untouched — no parsing, no
  buffering, no rewriting, ever. That is the design rule, and it is what makes
  a high number achievable at all:
  - **Opaque body pass-through** — pure relay after the head verdict. This is
    the row that can plausibly approach line rate, and the bench must confirm
    the body path performs no per-byte work beyond the copy.
  - **Inspected content** — HTML only, and only from Phase 4. Budget it
    separately and do not let it set expectations for the row above.

  Measure both on-device before writing either number; the earlier
  "saturate 1 Gbps" note was an assumption, and 125 MB/s through userspace on
  a 1.4 GHz ARM core — on a box where the DNS engine alone reached 65.8 % of it
  under load (`p1.5-06-review.md`) — is exactly the kind of target that should
  come from a measurement rather than produce one.
- **RAM ceiling ≤128 MB is a claim to verify, not assume.** Phase 1.5 already
  sits near it (~104 MiB steady at the configured cache size), so state the
  post-Phase-2 figure with EasyList + policies loaded and say plainly whether
  it fits. If p2-03 already flagged the headroom, this row confirms or
  contradicts it — either outcome gets recorded.
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
