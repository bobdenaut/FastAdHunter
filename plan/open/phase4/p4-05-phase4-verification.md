# P4-05 — Phase 4 Verification

**Phase:** 4 · **Depends on:** p4-04 · **Model:** Opus

## Goal

Phase 4 proven: rewrite budgets written into PERFORMANCE.md and held by
benches, e2e coverage over real list samples, on-device RB5009 validation,
and docs telling users exactly what HTML filtering does and doesn't reach.

## Context

PERFORMANCE.md currently has no HTML budgets — this task sets them from the
p4-03/p4-04 bench evidence (budgets are commitments, so they land with the
numbers that prove them). There is no CI; gates and benches run locally, soak
runs on the RB5009.

## Scope

- PERFORMANCE.md: add Phase 4 budget rows — rewrite added latency per page
  (p99), rewrite throughput floor, pass-through overhead when HTML filtering
  is enabled but not applicable (must stay ~zero), selector-cache memory
  ceiling. Justify numbers from recorded bench results.
- Benches vs those budgets in `benches/` (criterion), comparing against the
  no-rewrite baseline; wire into the >10% regression rule.
- e2e integration tests (`tests/`): full stack — real EasyList cosmetic
  sample, page fetched through the proxy plain-HTTP and intercepted-HTTPS,
  ads hidden/removed, `#@#` exception restores an element, non-HTML asset
  byte-identical, list refresh mid-traffic swaps selectors without dropped
  responses.
- RB5009 validation: household soak with HTML filtering on — RAM delta,
  rewrite latency, selector-cache hit ratio recorded; results noted in the
  task completion notes.
- Docs: deployment guide updated — what HTML filtering covers (plain HTTP,
  intercepted clients) and what it can't (spliced HTTPS, CSP-restricted
  injection); README feature claim updated; ROADMAP.md Phase 4 boxes checked.

## Acceptance criteria

- Every new PERFORMANCE.md budget has a bench that exercises it, and all
  pass on the dev machine; on-device numbers recorded.
- e2e suite green, including the list-refresh-under-traffic test.
- No doc still describes HTML filtering as future work.
- Gates green.

## Out of scope

New functionality of any kind; extended/procedural cosmetics (backlog).

## Suggested prompt

> Read PERFORMANCE.md, plan/wip/phase4/p4-05-phase4-verification.md, and the
> p4-03/p4-04 bench results. Set the Phase 4 budgets from evidence, add the
> benches and e2e tests, run the RB5009 soak, and update every doc that still
> calls HTML filtering future work.
