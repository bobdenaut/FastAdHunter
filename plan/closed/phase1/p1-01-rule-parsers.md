# P1-01 — Rule Parsers

**Phase:** 1 · **Depends on:** phase0 · **Model:** Opus

## Goal

`fah-rules` parses all four list formats and classifies every rule as
DNS-applicable or inactive.

## Context

RULE_ENGINE.md §Supported formats + ADR-0003 (full-format parsing day one).
Parsing is pure string → structured rules; no I/O, no matcher yet.

## Scope

- Parsers with per-list format auto-detection: hosts, plain domain list,
  EasyList/uBlock syntax, AdGuard extensions (`$dnstype`, `$dnsrewrite`,
  `$client` — parsed, stored).
- Classification per RULE_ENGINE.md: DNS-applicable (`||domain^`,
  `@@||domain^`, hosts entries, domain lines, AdGuard DNS options) vs non-DNS
  (cosmetic `##`, URL-path, HTTP `$options`) — stored inactive with counters.
- Unparseable lines: skipped + counted (`parse_errors`); never reject a list.
- Fixtures: real-world excerpts of each format (OISD, EasyList, AdGuard DNS
  filter samples) under `crates/fah-rules/tests/fixtures/`.

## Acceptance criteria

- Each fixture parses with expected active/inactive/error counts (asserted).
- Zero regex on any per-line hot path candidate; parsing allocates per rule,
  never per query.
- Gates green.

## Out of scope

Matching/verdicts (p1-02), downloading (p1-03).

## Suggested prompt

> Read RULE_ENGINE.md, ADR-0003, CONTEXT.md, and
> plan/wip/phase1/p1-01-rule-parsers.md. Implement the four parsers with
> format auto-detection, rule classification and fixture tests.
