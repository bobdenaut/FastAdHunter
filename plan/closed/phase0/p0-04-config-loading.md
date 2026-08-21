# P0-04 — Config Loading

**Phase:** 0 · **Depends on:** p0-02, p0-03 · **Model:** Opus

## Goal

`fah-config` loads typed configuration with the documented precedence:
defaults < TOML file < `FAH__*` env vars.

## Context

CONFIGURATION.md is the contract: every option, default and mutability class
is specified there. API write-back and hot reload come with fah-api (Phase 1);
what settles now is the typed schema, the merge order, and first-boot behavior.

## Scope

- Typed config structs mirroring CONFIGURATION.md sections (`[engine]`,
  `[dns.listen]`, `[dns.blocking]`, `[dns.cache]`, `[dns.upstreams]`,
  `[rules]`, `[query_log]`, `[stats]`, `[api]`, `[log]`) with the documented
  defaults.
- Load order: built-in defaults → optional TOML file → `FAH__` env overrides
  (`__` = section separator, e.g. `FAH__DNS__CACHE__MAX_ENTRIES`).
- First boot: missing file → write the default TOML to the configured path
  (the `/config` volume in Docker), then proceed.
- Validation with precise errors (unknown keys rejected; bad values name the
  key and expected form).
- Unit tests: precedence, env parsing, first-boot file generation (tempdir),
  validation failures.

## Acceptance criteria

- Defaults in code match CONFIGURATION.md exactly (test asserts a sample).
- Depends only on L1 crates + `serde`/`toml` (and a small env-merge approach —
  hand-rolled is fine; avoid heavyweight config frameworks).
- Gates green workspace-wide.

## Out of scope

API key generation, TLS cert generation, hot reload / watch channels, API
write-back — those land with the binary and fah-api tasks.

## Suggested prompt

> Read CONFIGURATION.md fully and plan/wip/phase0/p0-04-config-loading.md.
> Implement typed config with defaults, TOML + FAH__ env precedence,
> first-boot file generation and validation, with unit tests proving each.
