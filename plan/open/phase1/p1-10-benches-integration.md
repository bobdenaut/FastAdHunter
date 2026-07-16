# P1-10 — Benches and End-to-End Integration

**Phase:** 1 · **Depends on:** p1-06, p1-09 · **Model:** Sonnet

## Goal

Prove PERFORMANCE.md budgets with criterion benches and exercise the whole
product end-to-end.

## Context

PERFORMANCE.md budgets are acceptance targets, not aspirations. `benches/` is
the dedicated bench crate; `tests/` holds workspace integration tests. Numbers
recorded here become the baseline all later phases regress against.

## Scope

- Benches (consolidate + extend those added in p1-02/05/08): full-pipeline
  in-process QPS (mock upstream), verdict latency at 1M domains, cache-hit
  latency, blocked-query latency, startup time from cached lists (1M domains),
  steady-state RSS under load.
- End-to-end integration test: boot the real binary with tempdir volumes +
  mock upstream; resolve an allowed domain (answer flows), a blocked domain
  (0.0.0.0, TTL 10), verify stats via the API, watch the query appear on the
  WS stream, rotate the API key, add a user rule and see the verdict flip
  without restart.
- Record actual numbers vs budget table in the task completion note
  (dev-machine numbers; RB5009 numbers come from p1-11).

## Acceptance criteria

- All budget-mapped benches exist and run via `cargo bench`.
- Dev-machine results meet or beat every budget that is hardware-independent
  (ruleset ≤40MB, allocation-free lookups); latency/QPS results recorded.
- End-to-end test green, ≤60s runtime, fully offline.
- Gates green.

## Out of scope

On-device measurements (p1-11), soak duration testing.

## Suggested prompt

> Read PERFORMANCE.md §Budgets and plan/wip/phase1/p1-10-benches-integration.md.
> Consolidate the bench suite to map 1:1 onto the budget table and write the
> offline end-to-end integration test described.
