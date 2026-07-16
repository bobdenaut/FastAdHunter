# P1-02 — Compiled Matcher

**Phase:** 1 · **Depends on:** p1-01 · **Model:** Opus

## Goal

Parsed rules compile into a matcher answering verdicts within the
PERFORMANCE.md budgets.

## Context

RULE_ENGINE.md §Verdicts + §Compiled matcher; PERFORMANCE.md budgets:
1M domains ≤ 40MB compiled, verdict < 1ms p99 (target: microseconds),
lookup allocation-free, O(labels). This is the make-or-break data structure —
bench it before anything depends on its internals.

## Scope

- Compile step: parsed DNS-applicable rules → compact matcher
  (domain hash tables / label trie — implementer's choice, budget-bound).
- Verdict semantics per RULE_ENGINE.md: allow > block > pass; `||domain^`
  and hosts semantics match subdomains; exact-domain rules don't.
- `$dnstype` filtering honored; `$dnsrewrite` verdict payload carried
  (answer synthesis happens in fah-dns).
- Returns decisive rule + list references (for query log / rules/test).
- Criterion benches in `benches/`: compile time + memory for 1M synthetic
  domains, lookup latency distribution.
- Property tests: verdict stable under list order permutation (within
  documented precedence).

## Acceptance criteria

- Bench: 1M domains compile ≤ 40MB resident, lookup p99 < 1ms on dev machine
  (record actual numbers in completion note).
- Lookup path: zero allocations (assert via bench or `dhat` spot check).
- Gates green.

## Out of scope

List refresh/swap plumbing (p1-03), `$client` activation (Phase 2).

## Suggested prompt

> Read RULE_ENGINE.md §Compiled matcher, PERFORMANCE.md budgets, and
> plan/wip/phase1/p1-02-compiled-matcher.md. Design the matcher for the
> budgets, implement verdict semantics exactly, and prove with criterion
> benches + property tests.
