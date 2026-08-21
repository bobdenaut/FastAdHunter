# P1-07 — Statistics and Query Log

**Phase:** 1 · **Depends on:** p1-04 · **Model:** Opus

## Goal

`fah-stats` consumes QueryEvents: fixed-size aggregates + bounded persisted
query log, wired in the binary.

## Context

ARCHITECTURE.md §Data & Persistence + ADR-0002 (no embedded DB) +
CONFIGURATION.md `[stats]` / `[query_log]`. The channel wiring lives in
`fastadhunter` (siblings never import each other).

## Scope

- Aggregates: totals, blocked count/percent, cache-hit percent, rolling 24h
  buckets, bounded top-N (domains, blocked domains, clients) — fixed memory.
- Client registry: source IP → first/last seen, per-client counters, optional
  name (persisted to `/data`).
- Query log: in-RAM ring (config `ring_entries`) + batched append-only
  segments on `/data` (flush every `flush_interval_seconds`), JSONL or compact
  binary (implementer's choice, document it); retention by age + size caps
  with auto-prune.
- Stats snapshot to `/data` every `snapshot_interval_seconds`; loaded on boot
  so restarts don't zero the dashboard.
- Query API for fah-api (internal handle): paginated, filters per API.md
  (`client`, `domain`, `verdict`, `from`/`to`, cursor).
- Binary wiring: bounded channel dns → stats; slow consumer drops (counter).
- Tests: aggregate math, retention pruning (tempdir), snapshot round-trip,
  ring semantics, filter pagination.

## Acceptance criteria

- Memory fixed under sustained synthetic load (no growth with query count).
- Kill -9 and restart: stats within one snapshot interval of pre-kill values.
- Gates green.

## Out of scope

HTTP endpoints (p1-09), Prometheus (p1-08).

## Suggested prompt

> Read ADR-0002, CONFIGURATION.md §[stats]+[query_log], API.md §queries shape,
> and plan/wip/phase1/p1-07-stats-querylog.md. Implement aggregates, query log
> with retention, snapshots, and the internal query handle, plus tests.
