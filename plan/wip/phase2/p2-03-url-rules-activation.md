# P2-03 — URL Rules Activation

**Phase:** 2 · **Depends on:** p2-00 · **Model:** Opus

> Pure `fah-rules` work — it needs no `fah-http` scaffold, so it can run in
> parallel with p2-01/p2-02 rather than behind them. It is **not** independent
> of `p2-00`: until the parser fix lands, real EasyList is misdetected and its
> URL rules never enter the index, so there is nothing here to activate.

## Goal

The inactive URL-path and HTTP `$options` rules parsed since Phase 1 compile
into request-level matchers.

## Context

ADR-0003 pays off here — but for **classification only, not storage**. Read its
correction note before scoping this task. `RuleKind::Inactive(InactiveReason)`
is a bare `Copy` enum with no payload, and `ParsedRule` deliberately drops the
line text, so nothing of `||paypal.com^*/pixel.gif` survives parsing except one
discriminant saying "URL pattern". What day-one parsing bought is that the
classification is correct and complete (as of `p2-00`) — **not** that the rules
are sitting in the index waiting for a flag.

So this task has three parts, not one: retain the pattern text for
`UrlPattern`/`HttpOption` rules, compile it into a second matcher tier, and
answer request verdicts from it. RULE_ENGINE.md gains a §HTTP matching section
(doc update in the same change). Budgets: request verdict must stay
sub-millisecond; no regex on the hot path (EasyList wildcards/separators
compile to automata/masks, not regex).

Retention is a change to a hot startup path — the comment on `ParsedRule`
records that carrying text cost an allocation, a copy and a free per rule
across the whole list. Retain it for the two variants this task activates, not
for every inactive rule: cosmetic rules are 24,368 of EasyList alone and belong
to Phase 4.

## Scope

- **Retain first.** Extend `RuleKind::Inactive` to carry the pattern text (and
  raw options) for `UrlPattern` and `HttpOption` only — the two variants this
  task activates. Cosmetic stays payload-free until Phase 4. Measure the
  startup cost of the extra allocations against the pre-change baseline and
  record it; `ParsedRule`'s comment explains why that path was kept allocation-
  free, and reversing it deserves a number rather than a shrug.
- Activate: URL pattern rules (`/ads/*`, `||domain^path`, `^` separator, `*`
  wildcard, anchors), exceptions (`@@`), and HTTP-relevant `$options`:
  `$script/$image/$stylesheet/$xmlhttprequest` (from request context),
  `$third-party/$first-party` (Referer/Host relation), `$domain=`, `$method`.
- Request-verdict API: `(url, method, resource-type hints, referer, client)`
  → Verdict + decisive rule/list (extends fah-model types if needed —
  CONTEXT.md updated).
- Cosmetic rules (`##`) stay inactive (Phase 4) — counters keep reporting.
- **Memory headroom: measured, and it is not the constraint.** The question was
  answered ahead of this task
  (`docs/code-review/p2-03-headroom-and-parser-findings.md`): the compiled URL
  tier for EasyList + EasyPrivacy models to **~1.03 MiB**, and enabling both
  lists costs **≈4.0 MiB** all-in once their DNS-active halves are counted
  (1.52 + 1.42 MiB, measured with the real `Matcher`). Against the ~24 MiB of
  headroom (128 MB budget − ~104 MiB steady RSS, DNS ruleset 21.9 MiB), that
  leaves ~20 MiB. The modelling method over-estimates by 11.3 % when calibrated
  against the existing domain matcher, so this is an upper bound.
  Re-measure against the real implementation to **confirm** the figure — but
  treat it as confirmation, not as a gate. The Phase-2 memory risk lives in
  p2-02's connection pools and p2-05's per-policy ruleset duplication, not
  here.
- Compile-time budget guard: matcher memory measured with full EasyList in
  `benches/`; lookup allocation-free.
- Fixture tests: real EasyList excerpts with known-blocked/known-passed URL
  cases (borrow expectations from adblock-rust test vectors where license
  permits, else hand-derive).

## Acceptance criteria

- Full EasyList compiles; request verdict p99 < 1ms, allocation-free
  (bench-proven, numbers recorded).
- **Compiled URL-matcher heap measured and recorded as an absolute number**,
  with the remaining headroom against 128 MB stated explicitly. The predicted
  figure is ~1.03 MiB (upper bound); a real measurement materially above it
  means the compiled layout diverged from the modelled one — a finding to
  raise and explain, not a gate to quietly relax.
- `@@` exceptions beat blocks; `$domain=`/party options honored (tests).
- RULE_ENGINE.md documents HTTP matching in the same change.
- Gates green.

## Out of scope

Wiring into the proxy (p2-04), `$client` (p2-05), cosmetic/HTML (Phase 4).

## Suggested prompt

> Read RULE_ENGINE.md, ADR-0003 **including its correction note**,
> PERFORMANCE.md budgets, and plan/wip/phase2/p2-03-url-rules-activation.md.
> Note that inactive rules retain no text today — add retention for
> UrlPattern/HttpOption first, then build the request-level matcher tier
> without regex, update RULE_ENGINE.md, and prove budgets (including the
> startup cost of retention) with benches + fixture tests.
