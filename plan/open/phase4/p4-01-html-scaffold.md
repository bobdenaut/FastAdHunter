# P4-01 — HTML Filtering Scaffold and Doc Updates

**Phase:** 4 · **Depends on:** phase3 · **Model:** Sonnet

## Goal

HTML filtering exists as a configured, gated, documented subsystem before any
rewriting code lands: `[html]` config section, operating-mode gating, and every
governing doc reflects Phase 4 reality.

## Context

Root CLAUDE.md: a change contradicting the docs updates the doc in the same
change. HTML rewriting is response-body processing inside the HTTP pipeline —
it lives in `fah-http` (L3), consuming compiled cosmetic selectors from
`fah-rules` (L2). No new crate: an L3 sibling could not be imported by
fah-http (siblings never import each other), and the rewriter is meaningless
outside the proxy. lol_html is the fixed tech choice (root CLAUDE.md).

## Scope

- `lol_html` dependency added to `fah-http` (workspace-level version pin).
- CONFIGURATION.md + `fah-config`: `[html]` section — `enabled` (runtime,
  default follows operating mode), `max_selector_cache` entries (bounded,
  runtime), rewrite size/time guards if any land later (leave room, don't
  invent knobs yet).
- Gating: HTML filtering active only when the HTTP engine runs
  (`dns+http` / `dns+http+https`) **and** `html.enabled` — plain `dns` mode
  carries zero lol_html footprint at runtime.
- Doc updates in the same change:
  - ARCHITECTURE.md: HTML rewriting stage in the HTTP pipeline section
    (streaming, applied selectively, never buffers whole documents).
  - CONTEXT.md: **Cosmetic Rule**, **HTML Rewriting** terms; update the
    non-DNS rule wording (cosmetic rules no longer "inactive until the
    HTTP/HTML phases").
  - docs/diagrams: add the rewrite stage (keep layout).
- Rewrite hook stub in the fah-http response path: a no-op pass-through
  seam where p4-04 will attach the rewriter, with a test proving the seam
  does not copy or buffer bodies.

## Acceptance criteria

- Docs and diagram consistent (grep: no doc still claims cosmetic rules are
  inactive/parsed-only).
- `engine.mode = "dns"` ⇒ no HTML code path reachable; `html.enabled = false`
  ⇒ responses byte-identical through the proxy (test).
- Gates green.

## Out of scope

Any selector compilation (p4-02), any actual rewriting (p4-03).

## Suggested prompt

> Read root CLAUDE.md hard rules, ARCHITECTURE.md (HTTP pipeline),
> CONFIGURATION.md, and plan/wip/phase4/p4-01-html-scaffold.md. Add the
> `[html]` config section and mode gating, update every listed doc + diagram
> in the same change, and land the no-op rewrite seam with tests.
