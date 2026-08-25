# P5-04 — Dashboard and Lists

**Phase:** 5 · **Depends on:** p5-03 · **Model:** Opus

## Goal

The two pages that prove the shell can do everything the rest of the phase
needs: the Dashboard exercises tiles, time series, a donut, top-N tables and the
live stats push; Lists exercises mutation, confirmation, per-row failure states
and a synchronous long-running action.

## Context

Layout and content are settled in
[information-architecture.md](../../../docs/dashboard/information-architecture.md)
sections Dashboard and Lists, and drawn in
[docs/dashboard/sketch/](../../../docs/dashboard/sketch/) as `Main.dc.html` and
`Lists.dc.html`. The sketch settles arrangement, not implementation, and is
desktop-width only — the phone layout is this task's to design.

## Scope

**Dashboard** — `GET /stats` once on connect, then the stats push from the
socket; `GET /history/summary` for the series; telemetry for the ruleset and
upstream cards.

- Two tile rows: DNS (queries, blocked, blocked %, cache hit %) and HTTP plus
  engine (requests, blocked, compiled rules, uptime). The two pipelines are
  never summed into one "queries" figure.
- Queries over time, stacked allowed and blocked, with the 24 h / 7 d / 30 d
  range selector mapping to hourly then daily resolution. When the response
  carries a stride above 1, the chart says the series is decimated and that
  every plotted point is a real reading.
- Query-types donut; upstream attempts and failures as bars, never a
  share-of-traffic pie.
- Top queried, top blocked, top clients, cache state, ruleset card.

**Lists** — `GET /lists` plus the full CRUD and both refresh routes.

- The three-way rule partition per row as a stacked bar, with the meaning of
  inactive available and not implying breakage.
- Per-row status and error surfaced; failed and rejected rows say the last good
  copy still serves.
- Add by URL or mounted path, enable and disable, interval, remove.
- Refresh one is accepted and reports later through a list-refreshed event;
  refresh all is synchronous and blocking with per-list results. The two behave
  differently and the UI must not pretend otherwise.
- A conflict on add is rendered as what it is: a derived-id collision, or the
  same source already configured under another id, naming that list.
- A list-refreshed event re-reads the inventory; the event carries no reason, so
  the reason comes from the per-list error field.

**Mobile.** `sketch/MobileDashboard.dc.html` and `sketch/MobileNav.dc.html` are
the source of truth for the phone layout — build to them, not to a shrunken
desktop grid. They settle: tiles two-up rather than four-up; the range selector
as full-height chips, since pinch-zoom on the chart is not a discoverable
gesture; top-N tables as rows with the domain given the space and the count
right-aligned; a "show all" row rather than a scroller; the sidebar as an
overlay drawer with a scrim, 44 px rows, and connection state plus sign-out in
its footer.

Lists has no phone artboard yet: apply the same rule the drawn ones establish —
**one card per list, never a horizontal table**. That is the page an operator
reaches for when a list has failed, and a seven-column scroller is not an
answer. Draw it into the sketch as part of this task.

## Acceptance criteria

- Every figure on both pages traces to a documented API field. Nothing derived
  that the API does not support.
- The Dashboard updates from the socket push without polling stats.
- Range switching refetches correctly; a decimated response is labelled.
- An empty history window renders as "no data in this range", not an error.
- Refresh-all blocks with visible progress and reports per-list outcomes
  including rejected.
- A conflicting add shows the API's message and names the other list.
- Both pages correct in both themes at all three breakpoints, verified at
  390 px wide as well as desktop. No horizontal body scroll at any width.
- The Dashboard at 390 px matches `sketch/MobileDashboard.dc.html` in structure
  — tile pairing, chip-based range selector, row-form top-N tables — and the
  drawer matches `sketch/MobileNav.dc.html`.
- Lists at 390 px is one card per list, with an artboard added to the sketch.
- Touch targets on interactive controls are at least 44 px.
- Bundle still inside budget after uPlot lands. Size recorded.
- Gates green, cargo and frontend.

## Out of scope

Every other page. Long-range top-N is used only where the IA says.

## Suggested prompt

> Read docs/dashboard/information-architecture.md sections Dashboard and Lists,
> docs/dashboard/capability-matrix.md, API.md, and
> plan/wip/phase5/p5-04-dashboard-and-lists.md. Build both pages against the
> real API, driving the Dashboard from the socket's stats push, handle the list
> failure states and both refresh routes with their real semantics, and design
> the phone layout for both.
