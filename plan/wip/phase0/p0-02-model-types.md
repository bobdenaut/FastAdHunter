# P0-02 — Domain Model Types

**Phase:** 0 · **Depends on:** p0-01 · **Model:** Sonnet

## Goal

`fah-model` holds the shared domain types, pure and tested.

## Context

CONTEXT.md defines the vocabulary; ARCHITECTURE.md fixes the purity rule:
fah-model contains only data types and trivial traits — no business logic, no
I/O, no parsers, no cache. Every upper crate will consume these types, so their
shape settles first.

## Scope

- Types (names per CONTEXT.md): `Query` (domain, qtype, client IP, timestamp),
  `Verdict` (`Allow` / `Block` / `Pass`, with decisive rule + list references),
  `Client` (IP, optional name, first/last seen), `QueryEvent` (the channel DTO:
  query + verdict + duration + cache/upstream flags), `OperatingMode`
  (`Dns`, `DnsHttp`, `DnsHttpHttps`).
- Derives only: `Debug`, `Clone`, `PartialEq`, `serde` where DTOs need it.
- Unit tests: serde round-trips, `OperatingMode` parsing from config strings.

## Acceptance criteria

- `fah-model` has zero dependencies besides `serde` (and std).
- All types documented with doc-comments using CONTEXT.md wording.
- Gates green workspace-wide.

## Out of scope

DNS wire types (Hickory's `hickory-proto` provides those later, in fah-dns),
any matcher or rule types beyond what `Verdict` references.

## Suggested prompt

> Read CONTEXT.md, ARCHITECTURE.md §Dependency Layering (fah-model purity
> rule), and plan/wip/phase0/p0-02-model-types.md. Implement the types with
> serde derives and unit tests. No logic beyond trivial constructors/Display.
