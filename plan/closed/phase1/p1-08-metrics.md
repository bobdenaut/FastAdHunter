# P1-08 — Prometheus Metrics

**Phase:** 1 · **Depends on:** p1-04 · **Model:** Opus

## Goal

`fah-metrics` exposes operational telemetry in Prometheus text format.

## Context

CONTEXT.md distinguishes Metrics (ops) from Statistics (product). fah-metrics
is a sibling: it consumes the same QueryEvent channel plus counters other
crates expose; the `/metrics` HTTP route itself is served by fah-api (p1-09)
from this crate's registry.

## Scope

- Registry + instruments: query counters by verdict, latency histograms
  (in-engine, per stage), cache hit/miss/stale, upstream success/failure by
  server, channel-drop counters, ruleset size/compile time gauges, process
  RSS gauge.
- Zero-cost when unobserved: atomic counters on the hot path only — no locks,
  no allocation per event.
- Encoder to Prometheus text exposition format (use the `prometheus` crate or
  hand-encode — smallest dependency wins, justify choice).
- Tests: counter/histogram correctness, exposition format golden test.

## Acceptance criteria

- Hot-path instrumentation cost measured in `benches/` (should be low
  single-digit ns per event — record it).
- Exposition output scrapes clean with `promtool check metrics` semantics
  (well-formed names, HELP/TYPE lines).
- Gates green.

## Out of scope

The HTTP endpoint (p1-09), Grafana dashboards.

## Suggested prompt

> Read CONTEXT.md (Metrics vs Statistics), ARCHITECTURE.md wiring rules, and
> plan/wip/phase1/p1-08-metrics.md. Implement the registry, hot-path-safe
> instruments and the text encoder with tests + instrumentation-cost bench.
