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
