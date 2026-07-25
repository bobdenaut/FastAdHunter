# P2-03 — URL Rules Activation

**Phase:** 2 · **Depends on:** phase1 · **Model:** Opus

> Pure `fah-rules` work — it needs no `fah-http` scaffold, so it can run in
> parallel with p2-01/p2-02 rather than behind them.

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
- **Memory headroom is the real constraint — measure it first, before building
  the matcher out.** Measured on the RB5009: DNS ruleset 21.9 MiB, steady RSS
  ~104 MiB with the cache at its configured 50 000 / 64 MiB, against a 128 MB
  budget. That leaves **~24 MiB** for the URL matcher *plus* the HTTP engine
  and its pools (p2-02) *plus* per-policy rulesets (p2-05). "Full EasyList
  compiles" is therefore not free, and finding out at p2-07 is too late.
  Take a measurement of the compiled URL-matcher heap early and report it; if
  it does not fit, the options (subset the lists, share storage with the domain
  index, raise the budget with justification) are a decision for the user, not
  a silent overrun.
- Compile-time budget guard: matcher memory measured with full EasyList in
  `benches/`; lookup allocation-free.
- Fixture tests: real EasyList excerpts with known-blocked/known-passed URL
  cases (borrow expectations from adblock-rust test vectors where license
  permits, else hand-derive).

## Acceptance criteria

- Full EasyList compiles; request verdict p99 < 1ms, allocation-free
  (bench-proven, numbers recorded).
- **Compiled URL-matcher heap measured and recorded as an absolute number**,
  with the remaining headroom against 128 MB stated explicitly (see the
  ~24 MiB figure above). A number that does not fit is a finding to raise, not
  a gate to quietly relax.
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
