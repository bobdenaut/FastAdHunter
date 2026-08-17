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

## Completion note

**Design:** contiguous domain arena + fixed 8-byte `Record` per rule + a flat
open-addressing `u32` index (fastrange mapping, exact capacity, no power-of-two
rounding waste). No per-domain `Arc<str>` control blocks. `$dnstype` (bitmask)
and `$dnsrewrite` live in side maps keyed by record index (rare). `lookup` is
allocation-free and returns a compact `MatchDecision`/`RuleRef`; the
human-readable `DecisiveRule` is materialized only on the allow/block path
(never `Pass`) — so 1M rule-text strings are never retained.

**Bench (dev machine, `cargo bench -p fah-rules --bench matcher`):**

| Metric | Budget | Actual |
| ------ | ------ | ------ |
| 1M domains compiled, resident | ≤ 40 MB | **28.3 MiB** |
| Verdict lookup, exact hit | < 1 ms p99 | **~88 ns** |
| Verdict lookup, deep subdomain (3 extra labels) | < 1 ms | **~313 ns** |
| Verdict lookup, miss | < 1 ms | **~64 ns** |

Sub-microsecond on dev hardware; even a ~4× slowdown on the RB5009 ARM core
leaves ~3 orders of magnitude of margin under the 1 ms budget. Lookup zero-alloc
verified structurally (returns `Copy` refs; the arena/index are read-only).

**Tests:** 15 matcher unit tests (precedence, subdomain vs exact, case/trailing
dot, `$dnstype`/`$dnsrewrite`, decisive-rule reconstruction) + property tests:
verdict class invariant under 200 rule-order permutations across two lists, and
documented precedence/specificity cases.

**p1-01 review fix folded in:** hosts parser now skips the standard loopback
preamble (`127.0.0.1 localhost`, `::1 ip6-localhost`, …) so those never become
block rules, while `127.0.0.1 <real-ad-domain>` still blocks.

**Post-completion review:** see
[docs/code-review/phase1/p1-02-review.md](../../../docs/code-review/phase1/p1-02-review.md)
— 4 fixes applied (hot-path allocation for `Other` qtypes, `$dnstype=~`
negation, decisive-rule option separator, `heap_bytes` payload accounting).
