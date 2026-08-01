# Parse all rule-list formats fully from day one

Phase 1 is DNS-only, so it would be natural to parse only hosts files and
domain lists and defer the EasyList/uBlock/AdGuard family to the HTTP phase.
Instead the Rule Engine parses all four formats completely from day one and
classifies every rule as DNS-applicable (active) or non-DNS (stored inactive,
counted, activated by later phases).

Why: the parser is designed once instead of retrofitted mid-project; users can
load the very popular AdGuard-style DNS lists (`||domain^`, `$dnstype`,
`$dnsrewrite`) immediately; and Phases 2–4 activate already-parsed rules
instead of introducing new parsing risk. The cost — carrying inactive rules —
is bounded memory that the PERFORMANCE.md budgets already account for.

## Correction (2026-07-25, during p2-00)

**The decision stands; one sentence above overstates what was built.**
"Stored inactive" was not implemented. An inactive rule leaves behind its
`InactiveReason` and nothing else — `ParsedRule` deliberately drops the line
text, because retaining it cost an allocation, a copy and a free per rule on
the phase that dominates startup, for a field nothing read.

What day-one parsing actually bought is the part that matters: **classification
is correct and complete**, so a later phase knows precisely which rules belong
to it and never has to re-derive that. What it did not buy is free activation.
A phase that turns on a variant must reintroduce retention for it and pay the
memory — measured at ~1 MiB for EasyList + EasyPrivacy URL patterns
(`docs/code-review/p2-03-headroom-and-parser-findings.md`).

Read "Phases 2–4 activate already-parsed rules" as *already-classified* rules.
The distinction is worth keeping straight: it went unnoticed until `p2-00`, by
which point the optimistic reading had been copied into two task files.

**Settled by `p2-03` (2026-08-01).** The URL tier reintroduced retention and
paid the bill: **+3.32 ms** of parse across EasyList + EasyPrivacy (+16.7 %,
≈177 ns per retained rule) and **1.06 MiB** compiled for 18,778 rules — close
to the ~1 MiB predicted above. Both figures are measured, the parse delta
against the real pre-change parser (`docs/code-review/p2-03-review.md`).

The correction's practical consequence is now visible in the type itself:
`InactiveReason` carries only variants that genuinely stay inactive, and a
rule a tier *can* answer carries its payload. Phase 4 will pay the same bill
again for cosmetic rules, which remain payload-free deliberately — 24,368 in
EasyList alone.
