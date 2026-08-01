# P2-05 — Policy Model

**Phase:** 2 · **Depends on:** p2-03 · **Model:** Opus

## Goal

**Policy** becomes real: named bundle of rule lists + settings, assignable to
clients and schedules; `$client` rules activate.

## Context

CONTEXT.md reserved the term in Phase 1: "a named bundle of rule lists and
settings assignable to clients or schedules (parental-control style)".
This task defines the model + compilation; enforcement is p2-06. Docs updated
in the same change: CONTEXT.md (Policy un-reserved, Schedule term),
CONFIGURATION.md (`[[policies]]`), RULE_ENGINE.md (`$client` active).

## Scope

- Model (fah-model): `Policy` (id, name, list refs, setting overrides —
  blocking mode, safe-search hook left as no-op), `Schedule` (weekday/time
  ranges, e.g. "school nights 21:00–07:00"), `Assignment` (client → policy,
  optional schedule).
- Config schema `[[policies]]` + assignments; unassigned clients get the
  implicit **default policy** (current global behavior — zero-config users
  see no change).
- fah-rules: per-policy compiled rulesets sharing storage (lists referenced
  by multiple policies compile once — memory budget: N policies must not mean
  N× ruleset RAM; document the sharing design).
  **This is Phase 2's real memory risk — measure it before building it out.**
  The ruleset is 21.9 MiB against ~24 MiB of headroom, so *one* unshared copy
  overruns the 128 MB budget on its own. By contrast the URL matcher p2-03 was
  warned about costs ~1 MiB (`docs/code-review/p2-03-headroom-and-parser-findings.md`).
  Take an early measurement of two policies over overlapping lists and report
  the absolute heap before the model hardens; if sharing cannot hold the line,
  that is a decision for the user, not a silent overrun.
- `$client` rules activate: matched against client IP/name, effectively
  per-client inline policy.
- Schedule evaluation: pure function of (assignment, timestamp) — testable
  without clocks; DST-safe (test the ugly transitions).
- Tests: sharing memory shape, schedule edges, default-policy fallback.

## Acceptance criteria

- Two policies sharing one list: list compiled once (assert via size/pointer
  identity), **and the absolute heap recorded** — total for N policies over
  overlapping lists must stay within a few MiB of the single-ruleset figure,
  not a multiple of it.
- Schedule flips at boundaries correctly incl. DST (tests).
- Docs updated (CONTEXT/CONFIGURATION/RULE_ENGINE) in the same change.
- Gates green.

## Out of scope

Pipeline enforcement + API (p2-06).

## Suggested prompt

> Read CONTEXT.md §Policy, CONFIGURATION.md, RULE_ENGINE.md, and
> plan/wip/phase2/p2-05-policy-model.md. Implement Policy/Schedule/Assignment
> with shared-storage per-policy rulesets, activate $client, update the three
> docs, and cover the edge tests.
