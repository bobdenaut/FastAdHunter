# P5-07 — Settings and Diagnostics

**Phase:** 5 · **Depends on:** p5-06 · **Model:** Opus

## Goal

The System section: Settings, and the three Diagnostics views — Health, Memory
and Live Feed. The most opinionated pages in the phase, left until last on
purpose.

## Context

Two decisions are already closed in
[open-questions.md](../../../docs/dashboard/open-questions.md) section Closed and
must not be re-litigated: Diagnostics stays nested under System, and Settings is
hand-written per config section rather than generated. Sketched as `Settings`,
`Health`, `Memory`, `LiveFeed`.

## Scope

**Settings** — the config endpoints.

- Hand-written per config section, grouped as CONFIGURATION.md organises them.
  The API supplies current effective values and validation metadata; the
  frontend owns grouping, descriptions, and which keys appear.
- Every field tagged live-apply or restart-required. Most options are boot-only;
  the runtime set is small and named in the IA.
- A restart-required banner persists until a restart is observed through the
  health endpoint's uptime resetting. The config-changed event refreshes the
  form.
- The rule-list array and the policy set are absent from the form. They are
  rejected by this endpoint on purpose: each has exactly one writer, on its own
  page.
- **A read-only All settings panel at the bottom** rendering the complete
  effective config. A curated form is a subset by construction, and without this
  panel an operator cannot tell "not exposed here" from "not set".
- **Writes send only changed keys.** The endpoint is a partial deep-merge;
  submitting the read-back document would overwrite keys the UI does not model —
  a hand-edited value, or one from a newer build — with whatever the form last
  saw. The form tracks dirty state per field rather than diffing against a
  re-fetch, because the config-changed event can move the server's copy in
  between.
- Access panel: password change, API-key rotation with its shown-once warning,
  and sign-out-everywhere.

**Diagnostics · Health** — status, version, uptime; answer outcomes split
between synthesized and relayed; the shed counter, which covers both pipelines
as one number because they share one bounded channel; HTTP refusals, the only
standing signal of a LAN client probing; a summary of list problems. Degraded is
explained, not alarmed.

**Diagnostics · Memory** — the composition of RSS as a donut summing exactly to
RSS, with the residual rendered distinctly because it is a remainder rather than
a structure. A budget rail carrying current RSS, the lifetime peak as a
high-water marker, and the budget lines as markers rather than walls. The
persisted breakdown over time with residual as the top band, so a leak reads as
the top thickening while the components stay flat, and the peak series beside it
rather than derived from it. Page-fault rate as a derivative, never the
cumulative counter. Allocator figures as labelled values, marked as carrying no
compatibility promise.

**Diagnostics · Live Feed** — the event socket's query items. Columns for both
pipelines in one table. Client-side filters over the rows held. The panel states
that it starts empty, holds a bounded ring, and retains nothing — there is no
server-side query store to search. A cached marker is not a verdict.

**Mobile.** `sketch/MobileLiveFeed.dc.html` is the source of truth for the Live
Feed phone layout. It settles: one card per event, never the nine-column table —
verdict, pipeline and time on the first line, the domain given room to wrap,
everything else as metadata; a verdict-coloured left border so the feed is
scannable at a glance; filter chips in their own horizontal scroller while the
body never scrolls sideways; pause, clear and filter as full-width 44 px
controls; **a ring of 200 rows on a narrow viewport against 500 on desktop**;
and **rendering that stops while the page is hidden**, stated on the page,
resuming on return with whatever arrived meanwhile simply missed — there is no
history to backfill from.

Settings fields stack label-over-control. The memory rail and composition donut
stay legible without pinch-zoom.

## Acceptance criteria

- A settings write sends only the changed keys — asserted by inspecting the
  request body in a test.
- The All settings panel shows keys the curated form does not model.
- The restart banner appears on a boot-only change and clears after a restart.
- Rotating the API key warns that the key is shown once and that existing
  clients break.
- The memory donut's slices sum to RSS; peak is not among them.
- No memory or latency budget is drawn as a wall.
- The Live Feed ring is bounded, pauses when hidden, and never grows with
  uptime. At 390 px it matches `sketch/MobileLiveFeed.dc.html`: card per event,
  200-row ring, hidden-page pause stated on the page.
- Degraded status is explained identically wherever it appears — one string in
  the code, not three.
- Correct in both themes at all three breakpoints, verified at 390 px.
- Gates green, cargo and frontend. Bundle size recorded.

## Out of scope

Any config key the API rejects on that endpoint. Editing from the raw panel.

## Suggested prompt

> Read docs/dashboard/information-architecture.md sections Settings and
> Diagnostics, docs/dashboard/open-questions.md section Closed, API.md,
> CONFIGURATION.md for the sections being exposed, and
> plan/wip/phase5/p5-07-settings-and-diagnostics.md. Build the hand-written
> Settings form with its raw panel and changed-keys-only writes, and the three
> Diagnostics views. Design the phone layout for each, including the Live Feed
> as cards.
