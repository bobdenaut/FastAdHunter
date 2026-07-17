# P2-03 — URL Rules Activation

**Phase:** 2 · **Depends on:** p2-01 · **Model:** Opus

## Goal

The inactive URL-path and HTTP `$options` rules parsed since Phase 1 compile
into request-level matchers.

## Context

ADR-0003 pays off here: parsing exists, classification exists — this task
builds the second matcher tier. RULE_ENGINE.md gains a §HTTP matching section
(doc update in the same change). Budgets: request verdict must stay
sub-millisecond; no regex on the hot path (EasyList wildcards/separators
compile to automata/masks, not regex).

## Scope

- Activate: URL pattern rules (`/ads/*`, `||domain^path`, `^` separator, `*`
  wildcard, anchors), exceptions (`@@`), and HTTP-relevant `$options`:
  `$script/$image/$stylesheet/$xmlhttprequest` (from request context),
  `$third-party/$first-party` (Referer/Host relation), `$domain=`, `$method`.
- Request-verdict API: `(url, method, resource-type hints, referer, client)`
  → Verdict + decisive rule/list (extends fah-model types if needed —
  CONTEXT.md updated).
- Cosmetic rules (`##`) stay inactive (Phase 4) — counters keep reporting.
- Compile-time budget guard: matcher memory measured with full EasyList in
  `benches/`; lookup allocation-free.
- Fixture tests: real EasyList excerpts with known-blocked/known-passed URL
  cases (borrow expectations from adblock-rust test vectors where license
  permits, else hand-derive).

## Acceptance criteria

- Full EasyList compiles; request verdict p99 < 1ms, allocation-free
  (bench-proven, numbers recorded).
- `@@` exceptions beat blocks; `$domain=`/party options honored (tests).
- RULE_ENGINE.md documents HTTP matching in the same change.
- Gates green.

## Out of scope

Wiring into the proxy (p2-04), `$client` (p2-05), cosmetic/HTML (Phase 4).

## Suggested prompt

> Read RULE_ENGINE.md, ADR-0003, PERFORMANCE.md budgets, and
> plan/wip/phase2/p2-03-url-rules-activation.md. Build the request-level
> matcher tier without regex, update RULE_ENGINE.md, and prove budgets with
> benches + fixture tests.
