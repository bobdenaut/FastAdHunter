# P0-03 — Common Errors and Logging

**Phase:** 0 · **Depends on:** p0-01 · **Model:** Sonnet

## Goal

`fah-common` provides the shared error type; `fah-logging` initializes tracing
with configurable level and format.

## Context

L1 utilities the whole workspace leans on. CONFIGURATION.md `[log]` defines the
surface: `level` (error…trace, runtime-mutable later) and `format`
(`text` | `json`, boot-only). fah-common must not become a dumping ground —
errors and genuinely shared helpers only.

## Scope

- `fah-common`: `FahError` (thiserror), result alias, nothing else yet.
- `fah-logging`: `init(level, format)` building a `tracing-subscriber` stack;
  text and JSON formatters; returns a handle allowing later level changes
  (`reload` layer) so runtime mutability isn't a Phase 1 retrofit.
- Unit tests: init is idempotent-safe in tests, level filter honored,
  JSON output parses as JSON.

## Acceptance criteria

- Third-party deps limited to: `thiserror`, `tracing`, `tracing-subscriber`.
- No dependency on any sibling or upper crate.
- Gates green workspace-wide.

## Out of scope

Log rotation, file sinks, metrics — logging goes to stdout/stderr
(Docker-native).

## Suggested prompt

> Read CONFIGURATION.md §[log], CLAUDE.md hard rules, and
> plan/wip/phase0/p0-03-common-logging.md. Implement fah-common's error type
> and fah-logging's tracing init with a reloadable level filter and tests.
