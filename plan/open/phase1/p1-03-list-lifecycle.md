# P1-03 — List Lifecycle

**Phase:** 1 · **Depends on:** p1-02 · **Model:** Sonnet

## Goal

Rule lists flow: source → parse → validate → compile → atomic swap, with
`/data` caching and failure resilience.

## Context

RULE_ENGINE.md §List lifecycle + §Sources; ARCHITECTURE.md atomic-swap
principle. The hot path never locks; a failed refresh never degrades
protection.

## Scope

- Sources: remote URLs (reqwest/hyper via rustls), local files on `/data`,
  inline user rules.
- Refresh scheduler: per-list interval (default 24h from config), jittered;
  manual trigger hook (API calls it in p1-09).
- Atomic swap: `arc-swap` (or equivalent) of the compiled ruleset; in-flight
  queries finish on the old set.
- `/data` raw-copy cache; boot compiles from cache without network; async
  refresh after startup.
- Failure policy: download/validation failure → keep previous set, log,
  expose `last_status` per list.
- Default list (OISD basic) enabled on first run per CONFIGURATION.md.
- Tests: swap under concurrent lookups (loom or stress test), boot-from-cache,
  failure-keeps-previous.

## Acceptance criteria

- No lock on the lookup path (reads follow the current ruleset pointer).
- Kill-the-network test: refresh fails, verdicts unchanged, status surfaced.
- Gates green.

## Out of scope

API endpoints for lists (p1-09) — expose an internal handle they will call.

## Suggested prompt

> Read RULE_ENGINE.md §List lifecycle, CONFIGURATION.md §[rules], and
> plan/wip/phase1/p1-03-list-lifecycle.md. Implement sources, scheduler,
> atomic swap and /data caching with the failure-resilience tests.
