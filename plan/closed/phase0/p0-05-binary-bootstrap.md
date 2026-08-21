# P0-05 — Binary Bootstrap

**Phase:** 0 · **Depends on:** p0-03, p0-04 · **Model:** Opus

## Goal

`fastadhunter` starts: multi-threaded Tokio runtime, config loaded, logging
initialized, clean shutdown, and a working `--healthcheck` flag.

## Context

ARCHITECTURE.md §Runtime Model (the binary wires everything) and §Docker
(healthcheck via self-exec — distroless has no shell). Nothing to wire yet,
but the skeleton fixes startup order: config → logging → runtime → (future
engines) → shutdown signal handling.

## Scope

- CLI: `--config <path>` (default per CONFIGURATION.md), `--healthcheck`,
  `--version`.
- Startup: load config (first-boot generation included), init logging from
  `[log]`, log effective config source + operating mode, then idle awaiting
  shutdown (SIGTERM/ctrl-c) — exit 0 on signal.
- `--healthcheck`: for Phase 0, probe = process-level self-check (config loads,
  exits 0/1 with one log line). Marked with a TODO to switch to the API
  `GET /health` probe when fah-api lands (Phase 1).
- Integration test in `tests/`: spawn the binary with a temp config,
  assert healthcheck exit codes for good and broken config.

## Acceptance criteria

- `cargo run -- --healthcheck` exits 0 on a fresh tempdir config.
- Broken TOML → nonzero exit, error names the offending key/line.
- Graceful shutdown on ctrl-c within 1s, exit 0.
- Gates green workspace-wide.

## Out of scope

DNS listeners, API server, channels between siblings — there are no siblings
running yet.

## Suggested prompt

> Read ARCHITECTURE.md §Runtime Model + §Docker healthcheck note,
> CONFIGURATION.md, and plan/wip/phase0/p0-05-binary-bootstrap.md. Implement
> the binary bootstrap with clap-or-hand-rolled CLI (prefer minimal deps),
> Tokio runtime, and the healthcheck flag, plus the integration test.
