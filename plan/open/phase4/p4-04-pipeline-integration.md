# P4-04 — Pipeline Integration and Selective Application

**Phase:** 4 · **Depends on:** p4-03 · **Model:** Opus

## Goal

The rewriter attaches to the p4-01 seam in the HTTP/HTTPS pipeline: HTML
responses that need it get rewritten, everything else keeps the pass-through
fast path byte-for-byte. Policies, events, stats and metrics know about it.

## Context

ROADMAP.md: "applied only where required; all other traffic passes through
untouched" — the gate is the feature. The rewriter reaches whatever flows
through the Phase 2 pipeline: plain HTTP and intercepted-client HTTPS
(p3-04); spliced HTTPS is untouchable by design. Policy concept (p2-05)
already assigns settings per client.

## Scope

- Application gate, all conditions required, checked cheapest-first:
  1. `html.enabled` and client's policy allows HTML filtering
     (new per-policy toggle — CONFIGURATION.md + policy API updated);
  2. response `Content-Type` is `text/html` (never sniff bodies);
  3. p4-02 lookup returns a non-empty selector set for the request hostname.
  Failing any ⇒ untouched pass-through fast path.
- Candidate-request handling per the p4-03 encoding decision (identity
  `Accept-Encoding` adjustment happens on the request side here).
- Framing correctness: rewritten responses drop `Content-Length` and use
  chunked/stream framing (HTTP/1.1) or native DATA framing (HTTP/2); never
  emit a stale length.
- Works identically for plain HTTP and the p3-04 interception path (same
  pipeline — no second code path).
- Observability, all bounded:
  - RequestEvent gains rewrite outcome (rewritten / passed / failed-open,
    selector count) — fah-model DTO stays logic-free;
  - fah-stats: rewritten-response counters in the rolling aggregates;
  - fah-metrics: rewrite counter + duration histogram + selector-cache
    hit ratio.
- API.md updated where event/stats shapes changed.
- Tests: each gate condition independently forces pass-through
  (byte-identical proof); rewritten page over plain HTTP and over intercepted
  HTTPS; policy with filtering off ⇒ untouched for that client while another
  client is rewritten; rewriter failure mid-stream fails open.

## Acceptance criteria

- Non-HTML and selector-less responses are byte-identical through the proxy
  with no added buffering (test + pass-through bench unchanged vs p2-02
  baseline within noise).
- Per-client policy toggle proven by the two-client test.
- CONFIGURATION.md + API.md updated in the same change.
- Gates green.

## Out of scope

Budget-setting and on-device proof (p4-05), extended cosmetics (backlog).

## Suggested prompt

> Read plan/wip/phase4/p4-04-pipeline-integration.md, the p2-04 pipeline and
> p3-04 interception code, and CONFIGURATION.md. Wire the p4-03 rewriter into
> the response path behind the three-condition gate, fix framing, add the
> policy toggle and observability, and prove every gate + fail-open with
> tests.
