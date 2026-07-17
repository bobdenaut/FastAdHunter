# P4-02 — Cosmetic Rules Activation

**Phase:** 4 · **Depends on:** p4-01 · **Model:** Opus

## Goal

Cosmetic rules (`##`, `#@#`, domain-scoped variants) stop being parsed-only:
fah-rules compiles them into per-hostname selector sets the rewriter can fetch
allocation-free, with exceptions honored and counts active in the API.

## Context

RULE_ENGINE.md: all formats fully parsed since day one (ADR-0003) — cosmetic
rules already exist as parsed, counted, inactive data. This task activates
them. All rule processing stays in fah-rules (there is no second rule engine);
fah-http only asks "selectors for hostname X?". Compiled sets live in the same
atomically-swapped ruleset as everything else — list refresh swaps cosmetic
selectors too, hot path takes no lock.

## Scope

- Compile at load time into: **generic** selectors (`##.ad`, apply everywhere)
  and **domain-specific** selectors (`example.com##.banner`, including
  subdomain semantics and `~domain` negation).
- Exceptions `#@#` subtract matching selectors per the same domain scoping;
  allow-over-block precedence analog documented in RULE_ENGINE.md.
- Lookup: given a hostname, return its effective selector set (generic +
  matching specific − exceptions). O(labels) probes, no allocation on lookup;
  set assembly may be pre-joined at compile time or cached — bound by the
  budgets p4-05 adds to PERFORMANCE.md.
- Extended/procedural cosmetics (`#?#`, `:has()`, scriptlets `#%#`, CSS
  injection `#$#`) remain parsed-but-inactive — counted separately so the API
  distinguishes "active cosmetic" from "unsupported cosmetic".
- API: per-list rule counts now report cosmetic rules as active;
  `POST /api/v1/rules/test` extended to dry-run a hostname's cosmetic
  selector set and name the deciding list/rule — API.md + RULE_ENGINE.md
  updated in the same change.
- Tests: scoping (domain, subdomain, `~` negation), exception subtraction,
  atomic swap mid-lookup, malformed selector lines skipped and counted as
  `parse_errors`.

## Acceptance criteria

- A hostname's effective selector set is correct for generic/specific/
  exception combinations (table-driven tests against real EasyList samples).
- No allocation on the lookup hot path (asserted via bench or counting
  allocator in tests).
- RULE_ENGINE.md + API.md updated in the same change.
- Gates green.

## Out of scope

Any HTML processing (p4-03), selector-to-lol_html translation (p4-03),
extended/procedural cosmetics (backlog).

## Suggested prompt

> Read RULE_ENGINE.md, CONTEXT.md, and
> plan/wip/phase4/p4-02-cosmetic-rules-activation.md. Activate cosmetic rule
> compilation in fah-rules with domain scoping and `#@#` exceptions, expose
> the per-hostname selector lookup and API counts, update both docs, and
> prove scoping with table-driven tests.
