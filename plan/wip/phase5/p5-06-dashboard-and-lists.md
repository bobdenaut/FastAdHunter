# P5-06 — Dashboard and Lists

**Phase:** 5 · **Depends on:** p5-05 · **Model:** Opus

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
socket; `GET /history/summary` for the series; the shared bounded refresh from
`p5-05` for the telemetry-backed cards.

- **The socket subscribes to `stats` only, and only while the page is mounted.**
  The Dashboard renders no per-query rows, so it must not receive `query` events;
  leaving the page releases `stats`, which closes the connection unless Lists or
  Settings is what was navigated to.
- **The stats push covers exactly half the page.** `encode_stats` sends the
  `GET /stats` payload — tile row 1, the top-N tables, the buckets. The ruleset
  card, the upstream bars, the HTTP tiles, uptime and cache state come from
  `/telemetry`, `/health` and `/cache`, **none of which the socket pushes**.
  Those go through the shared bounded refresh on its slow interval, paused while
  the page is hidden. Nothing on this page runs a timer of its own.
- Two tile rows: DNS (queries, blocked, blocked %, cache hit %) and HTTP plus
  engine (requests, blocked, compiled rules, uptime). The two pipelines are
  never summed into one "queries" figure.
- **The HTTP tiles are labelled "since restart".** `/stats` is a rolling 24 h
  window; `counters.http` is process-lifetime cumulative and returns to zero on
  restart (API.md §telemetry). Two visually identical tile rows meaning different
  windows is the trap this label exists to close. There is no 24 h HTTP figure in
  the API — do not derive one.
- Queries over time, stacked **permitted** and blocked, with the 24 h / 7 d / 30 d
  range selector mapping to hourly then daily resolution. When the response
  carries a stride above 1, the chart says the series is decimated and that
  every plotted point is a real reading.
  - **`permitted` is `queries − blocked`, and it is not `allow`.**
    `history/summary` items carry `queries`, `blocked`, `cache_hits` and
    `per_type` — there is no allowed series. `allow` in CONTEXT.md is an explicit
    exception match, four figures against six-figure traffic; labelling the
    derived band "allowed" would state something the engine never measured.
  - **The series is DNS-only** and the chart says so. `history/summary` carries
    no HTTP data, and this page insists everywhere else that the pipelines are
    not conflated.
- **`history.enabled = false` is its own state, not an empty chart.** The flag is
  runtime-mutable, so Settings can switch it off live and `/history/*` then keeps
  answering `200` with empty `items` forever. Read the flag and render "history is
  disabled" — an empty range and a disabled recorder must not look identical.
- Query-types donut; upstream attempts and failures as bars, never a
  share-of-traffic pie.
- Top queried, top blocked, top clients, cache state, ruleset card.

**Lists** — `GET /lists` plus the full CRUD and both refresh routes.

- The three-way rule partition per row as a stacked bar, with the meaning of
  inactive available and not implying breakage. `parse_errors` is shown beside
  it: it describes the copy currently serving, not a refused body, and it reads
  `0` both for a clean list and for one contributing nothing — so it is read
  beside `enabled` and `rules_total`, and the UI presents it that way.
- Per-row status and error surfaced across all five values —
  `ok` | `degraded` | `failed` | `rejected` | `never`. Failed and rejected rows
  say the last good copy still serves.
  - **`degraded` is not a milder `ok`.** It means the fetch succeeded but most of
    the body failed to parse — the signature of a format misdetection, where the
    list contributes far fewer rules than it should. It gets its own treatment
    and points at RULE_ENGINE.md §Supported formats.
  - **A `rejected` row carries its recovery.** The content gate refuses a body
    against the cached baseline, and a source that legitimately restructured stays
    rejected on every attempt, across restarts. Disabling and re-enabling does
    **not** clear it. The API's recovery contract is `DELETE` then re-add, and
    that is the action the row offers — this is the page an operator opens when a
    list is broken, so the way out belongs on it.
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
  that the API does not support, and the one derived figure that is allowed —
  `permitted` — is labelled as itself, never as `allow`.
- The Dashboard updates from the socket push without polling `/stats`, and its
  telemetry-backed cards refresh through the shared bounded mechanism, not their
  own timers. No `query` events reach this page.
- **Route-scoped, per the phase invariant.** Each page holds `stats` (Dashboard)
  or `list_refreshed` (Lists) only while mounted, and neither issues a request
  after being navigated away from. Asserted with a request log: leave either page
  and its traffic goes to zero within one refresh interval.
- The HTTP tiles are labelled "since restart" and the primary chart is labelled
  DNS-only.
- Range switching refetches correctly; a decimated response is labelled.
- An empty history window renders as "no data in this range", not an error — and
  `history.enabled = false` renders as "history disabled", distinctly.
- Refresh-all blocks with visible progress and reports per-list outcomes
  including rejected.
- A `degraded` row is distinguishable from `ok`; a `rejected` row offers the
  `DELETE`-and-re-add recovery rather than a disable/enable that cannot work.
- A conflicting add shows the API's message and names the other list.
- Both pages correct in both themes at all three breakpoints, verified at
  390 px wide as well as desktop. No horizontal body scroll at any width.
- The Dashboard at 390 px matches `sketch/MobileDashboard.dc.html` in structure
  — tile pairing, chip-based range selector, row-form top-N tables — and the
  drawer matches `sketch/MobileNav.dc.html`.
- Lists at 390 px is one card per list, with an artboard added to the sketch.
- Touch targets on interactive controls are at least 44 px.
- Bundle still inside budget after uPlot lands, and uPlot is in the chart chunk —
  not in the shell or login chunk. Size recorded, gzip and brotli.
- Gates green, cargo and frontend.

## Out of scope

Every other page. Long-range top-N is used only where the IA says.

## Suggested prompt

> Read docs/dashboard/information-architecture.md sections Dashboard and Lists,
> docs/dashboard/capability-matrix.md, API.md, and
> plan/open/phase5/p5-06-dashboard-and-lists.md. Build both pages against the
> real API, driving the Dashboard from the socket's stats push and the shared
> bounded refresh for everything the socket does not carry, labelling the HTTP
> tiles "since restart" and the primary series DNS-only and `permitted`, handling
> `degraded` and `rejected` lists with their real recovery, and design the phone
> layout for both.
