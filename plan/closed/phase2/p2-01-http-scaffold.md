# P2-01 — HTTP Crate Scaffold and Doc Updates

**Phase:** 2 · **Depends on:** phase1 · **Model:** Opus

## Goal

`fah-http` exists as an L3 crate, and every governing doc reflects Phase 2
reality before code lands.

## Context

Root CLAUDE.md: a change contradicting the docs updates the doc in the same
change. Adding an engine crate touches ARCHITECTURE.md (layout + layering),
CONTEXT.md (new terms), CONFIGURATION.md (new section), README (component box
already lists HTTP Engine — verify).

## Scope

- New crate `crates/fah-http` (L3 — depends on fah-rules and L1 only; sibling
  of fah-dns/fah-api/fah-stats/fah-metrics; wired by the binary).
- Doc updates in the same change:
  - ARCHITECTURE.md: crate list + layering diagram + HTTP pipeline section
    (streaming, pass-through fast path, transparent interception).
  - CONTEXT.md: **HTTP Engine**, **Pass-through**, **Interception** terms;
    sharpen **Operating Mode** wording if needed.
  - CONFIGURATION.md: `[http]` section — `enabled` via `engine.mode`,
    `listen.port` (default 8080, boot), timeouts, max concurrent connections
    (bounded, runtime).
  - docs/diagrams/architecture.svg + .html: add fah-http (keep layout).
- Operating mode plumbing: `dns+http` starts the (still empty) HTTP engine;
  `dns` mode must not bind its port.
- Stub listener accepting and immediately closing connections, with tests.

## Acceptance criteria

- Docs and diagram consistent with the new crate (grep: no doc still claims
  9 L3-less crates).
- `engine.mode = "dns"` ⇒ port 8080 not bound; `"dns+http"` ⇒ bound.
- Gates green.

## Out of scope

Any proxying (p2-02), any filtering (p2-04).

## Suggested prompt

> Read root CLAUDE.md hard rules, ARCHITECTURE.md, CONFIGURATION.md, and
> plan/wip/phase2/p2-01-http-scaffold.md. Create fah-http with correct
> layering, update every listed doc + the diagram in the same change, and
> gate the listener on operating mode.
