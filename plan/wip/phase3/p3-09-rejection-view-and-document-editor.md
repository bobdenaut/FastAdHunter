# P3-09 — Rejection View and Interception Document Editor

**Phase:** 3 · **Depends on:** p3-07, p3-08 (ADR-0008 step 3) · **Model:** Fable

## Goal

The operator sees which hosts refuse the minted leaf, excludes exactly the host
observed with one click, and edits `clients` and `exclude_domains` without
touching a TOML or restarting anything. Every write is a whole-document
`PUT /api/v1/interception`.

## Context

[ADR-0008](../../../docs/decisions/0008-live-interception-and-client-certificate-rejection.md)
step 3 of §Phasing — read §The operator's path, §detect ≠ auto-exclude, §The
missing-CA case and §A long exclusion list. The Live Feed
(`dashboard/frontend/src/pages/live-feed.tsx`, `live-feed/filters.ts`) filters
by `kind` and verdict over the bounded event ring; `https` events with status
525 arrive through the same `WS /api/v1/events` socket. No dashboard surface
for `[https.interception]` exists today. Settings conventions:
docs/dashboard/information-architecture.md §Settings.

## Scope

- **The rejection view.** A Live Feed entry beside the `kind` filters —
  "Certificate rejected by client" — showing `https` events with status 525
  grouped by client and host, with a count and the last time seen; the raw
  stream and the ring stay as they are. One action per row: exclude the exact
  host observed — read the document, add the entry, `PUT` the whole document
  back. A duplicate or cap rejection is shown verbatim. No widening button, no
  default, no countdown, no bulk action; a host already excluded is shown as
  such rather than offered again.
- **The editor.** A Settings page for the Interception Document: `clients` and
  `exclude_domains` as editable lists, `GET` on open, one `PUT` on save with
  the document as displayed, validation errors rendered by list and entry, the
  caps stated. No restart banner: the response carries no `restart_required`,
  and the page says a saved change applies on the next connection. A mode
  without the HTTPS listener says the document is stored and inert until a
  mode with one boots.
- **Wording.** Never "pinned" — "Certificate rejected by client" and
  "exclusions"; the label states what was observed. Bounded: the grouping
  works over the ring the page already holds, no second buffer.
- **Tests.** `vitest` for the grouping, the exclude action's request body
  (whole document, exact host), duplicate and cap error rendering, the editor
  round trip; a request log asserting no polling was added.
- **Docs, same change.** docs/dashboard/information-architecture.md gains the
  two surfaces; API.md only if a shape detail surfaces; CONTEXT.md only if a
  new term appears.

## Acceptance criteria

- A rejected host appears once per client with a count; clicking exclude
  issues one `PUT` whose `exclude_domains` is the previous list plus exactly
  the observed host — proven by the request body, and end to end by the next
  connection splicing (harness or live origin).
- The button cannot exclude anything but the exact observed host.
- The editor saves what it displays; a rejected `PUT` leaves the page's
  document unchanged and shows the server's error.
- No `restart_required` anywhere on either surface; no timer added.
- Correct in both themes at all three breakpoints, verified at 390 px.
- Gates green, cargo and frontend (`npm run typecheck`, `npx vitest run`,
  `npm run build` within the 150 KB gzip budget); bundle size recorded.

## Out of scope

Widening to a parent (ADR-0008 §The operator's path — needs a public-suffix
list, declined); auto-exclusion; the missing-CA diagnosis and any `UnknownCA`
surface; editing anything else under `[https]`.

## Suggested prompt

> Read ADR-0008 §The operator's path and §detect ≠ auto-exclude,
> plan/wip/phase3/p3-09-rejection-view-and-document-editor.md,
> `dashboard/frontend/src/pages/live-feed.tsx`, API.md §`/api/v1/interception`
> and §`WS /api/v1/events`. Plan the grouping, the exclude action and the
> editor; wait for approval; then build both surfaces with the wording the ADR
> fixes.
