# P5-03 — Frontend Foundation

**Phase:** 5 · **Depends on:** p5-02 · **Model:** Opus

## Goal

The application shell exists and works: it signs in, holds a live socket,
renders the navigation and the card vocabulary in both themes at every
breakpoint, and fails the build if it grows past its budget. No product page
yet — everything after this task is a page and nothing else.

## Context

Root CLAUDE.md's layout note says `dashboard/` is empty until the dashboard
phase and must not be scaffolded. This task is that phase: the note is updated
in the same change.

Stack is fixed by
[docs/dashboard/visual-system.md](../../../docs/dashboard/visual-system.md):
TypeScript + Preact + Vite, uPlot for time series, hand-drawn SVG for donuts,
own CSS. No AdminLTE, Bootstrap, jQuery or DataTables. No CDN — the box may be
the network's only resolver, and a UI that needs the internet fails exactly when
it is needed.

## Scope

- **Project** at `dashboard/frontend/`: Vite + TypeScript + Preact, static
  build, output consumed by the `frontend` Docker stage from p5-01. Strict
  TypeScript.
- **Typed API client**, hand-written from API.md — one module per resource
  group, one error type from the documented envelope, unknown fields ignored
  rather than rejected. Hand-written on purpose: there is no OpenAPI document,
  and writing the types is how the contract gets read carefully.
- **Socket manager** for the events WebSocket: one connection per session,
  reconnect with backoff, dispatch of the four message types, exposed
  connection state. Disconnects are expected — slow consumers are dropped by
  design — so the indicator is informational, not an error. Authenticates by
  cookie; never by query-string token.
- **Login page** against p5-02, and a route guard that returns to it on 401.
- **Shell**: sidebar with the four sections and the nested Diagnostics group,
  top bar with connection state and version, content header, card grid.
- **Component vocabulary**, built once and used by every later task: tile, card,
  table with tabular figures and frequency bars, verdict pill, status pill,
  stage bar, empty state, error state, confirm dialog, chart wrapper.
- **Theme**: light and dark as tokens on the root, explicit toggle defaulting to
  the system preference. Colour is never the only signal.
- **Responsive**: the three breakpoints in visual-system.md. The body never
  scrolls horizontally; wide content scrolls inside its own container.
- **Self-hosted assets**: fonts and a small SVG sprite. No icon font.
- **Size gate**: the build fails over 150 KB gzip, wired into the build script.
- **Doc update in the same change**: root CLAUDE.md's layout line, since
  `dashboard/` is no longer empty.

## Acceptance criteria

- The build produces a static bundle that p5-01 serves unchanged.
- Typecheck passes with no implicit any.
- Signing in works end to end against a running fah-api; an expired session
  returns to the login page rather than showing an error.
- The socket connects, survives a forced disconnect, and reconnects with
  backoff. Connection state is visible.
- Shell renders correctly in both themes at all three breakpoints, with no
  horizontal body scroll.
- The size gate fails the build when deliberately exceeded — prove it once.
- Final bundle size recorded against the 150 KB budget.
- Cargo gates green: the workspace is untouched, but they still run.

## Out of scope

Every product page. Any chart bound to real data — the chart wrapper is proven
with a static series.

## Suggested prompt

> Read docs/dashboard/visual-system.md,
> docs/dashboard/information-architecture.md, API.md, and
> plan/wip/phase5/p5-03-frontend-foundation.md. Scaffold dashboard/frontend/
> with Vite, TypeScript and Preact, write the typed API client and the socket
> manager, build the shell and the shared component vocabulary in both themes,
> wire the login flow against p5-02, and add the bundle-size gate. Update root
> CLAUDE.md's layout note in the same change.
