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

## Completion note — measured 2026-07-19

Dev machine: Windows 11, x86-64. Benches pinned to one core (throughput to
four, matching the RB5009's core count) per PERFORMANCE.md §Measuring
reliably — unpinned runs on this box swing up to 6× and are not usable.

| Budget (PERFORMANCE.md) | Target | Measured | Headroom |
| ----------------------- | ------ | -------- | -------- |
| Compiled ruleset, 1M domains | ≤ 40 MB | **28.3 MiB** | 1.4× |
| Blocked query, in-engine | < 1 ms | **1.83 µs** | ~550× |
| Verdict + cache hit, in-engine | < 1 ms | **1.87 µs** | ~535× |
| Forwarded query overhead | < 1 ms | **2.44 µs** | ~410× |
| Sustained throughput (4 cores) | ≥ 10 000 QPS | **611 000 QPS** | 61× |
| Startup from cached lists, 1M | 1–3 s (~1 s goal) | **300 ms** | 3.3× under goal |
| RAM steady-state, 1M loaded | ≤ 128 MB | not measurable on Windows | p1-11 |

Component benches: matcher lookup 112 ns exact / 367 ns deep subdomain /
64 ns miss; metrics event record 18 ns.

Every hardware-independent budget is met. RSS needs `/proc`, so the 128 MB row
is deliberately left to the on-device run in p1-11 rather than faked.

### Finding: the matcher is no longer where the time goes

The budgets are met by two to three orders of magnitude, so the interesting
number is not the headroom but the *split*:

| path | total | matcher share | rest |
| ---- | ----- | ------------- | ---- |
| blocked | 1.83 µs | 367 ns | ~80% |
| cache hit | 1.87 µs | 64 ns | ~97% |
| forwarded | 2.44 µs | 64 ns | ~97% |

On the cache-hit path the compiled matcher is **3%** of the query. The
remaining ~1.8 µs is DNS packet decode, the cache lookup itself, response
synthesis and encode, and the `QueryEvent` channel send. Further work on the
matcher buys almost nothing; the profitable targets in later phases are packet
parse/serialize, cache lookup, the tokio socket path, lock contention, and
query logging. Worth a decomposition bench before optimizing any of them —
this split is inferred from separate benches, not measured in one profile.
