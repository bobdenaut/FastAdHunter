# P5-09 — Settings and Diagnostics

**Phase:** 5 · **Depends on:** p5-08 · **Model:** Opus

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
- **`GET /config` supplies current effective values and nothing else.** It is not
  a schema endpoint: API.md describes it as "effective configuration (all sources
  merged), secrets redacted", and the handler serialises the typed config
  verbatim. There is no validation metadata — no types, no bounds, no enums, no
  mutability classes. Every one of those is hand-carried from CONFIGURATION.md
  and the backend schema into the frontend, which is a cost this page pays on
  purpose and a drift risk it accepts. Anything that reads as "the API told us
  the bounds" is wrong.
- Every field tagged live-apply or restart-required. Most options are boot-only;
  the runtime set is small and named in the IA.
- **`[api]` is not an ordinary editable section.** `api.tls = false` removes the
  only origin on which a `Secure` `__Host-` cookie can exist — after that restart
  the dashboard cannot authenticate at all, and an HTTP fallback is forbidden.
  `api.address` and `api.port` move the listener out from under whoever is using
  it. Either exclude the section from the curated form, or gate each field behind
  an explicit consequence confirmation naming the lock-out. Presenting TLS as a
  harmless toggle is the failure mode.
- A restart-required banner persists until a restart is observed through the
  health endpoint's uptime resetting. The config-changed event refreshes the
  form. The banner is **global UI state, not a global poll**: it revalidates on
  entering Settings and whenever a mounted page's shared refresh happens to read
  `/health`. No timer exists solely to clear it.
- The rule-list array and the policy set are absent from the form. They are
  rejected by this endpoint on purpose: each has exactly one writer, on its own
  page.
- **A read-only All settings panel at the bottom** rendering the complete
  effective config. A curated form is a subset by construction, and without this
  panel an operator cannot tell "not exposed here" from "not set".
  - **It renders whatever `GET /config` returns, so it must never render auth
    material.** `p5-04` redacts `auth.*` at the endpoint — that is the guarantee
    this panel relies on, and the panel adds a second check rather than assuming
    it. A password hash printed into a browser is offline-crackable material,
    and "it is only a hash" is not a reason to publish it.
  - `policies` is absent from the response when the list is empty
    (`skip_serializing_if`), so the panel shows no key at all in the zero-config
    case. Word it so that reads as "none configured", not as the ambiguity the
    panel exists to remove.
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

**Diagnostics · Memory** — `sketch/Memory.dc.html` is the source of truth for
this page. The composition of RSS as a 100 %-stacked bar summing exactly to RSS,
with the residual rendered as a texture rather than a hue because it is a
remainder rather than a structure, and the four components split into tiles
beneath it. A KPI rail of four figures — RSS, residual, lifetime peak, allocator
committed — each carrying its own trend and its own denominator, with the
budget lines as markers rather than walls and peak explicitly not measured
against the budget. The persisted breakdown over time with residual as the top
band, so a leak reads as the top thickening while the components stay flat, and
the peak series beside it rather than derived from it. Page-fault rate as a
derivative, never the cumulative counter. Allocator figures as labelled values,
marked as carrying no compatibility promise.

- **The RSS line carries a state, and the two thresholds are drawn.** Ink below
  100 MiB, red above the steady-state budget in MiB, with the band between them
  shaded and labelled. 100 MiB is the watch point because it sits above anything
  the sampled series has recorded, so a reading there is new territory while
  still under budget. Both thresholds are dotted rules with captions, never
  walls — and red is a status colour, so it ships with the labelled budget line
  that makes it legible, never alone.
- **The categorical palette is capped at what can actually be told apart.**
  Ruleset takes one hue, cache and stats two steps of a second, residual the
  texture, RSS the ink of the stack's top edge, peak the amber. A fourth
  categorical hue does not survive the colour-blind separation floor against the
  first three plus amber and red, which is why cache and stats share a hue and
  the review file records the measured separations.

**Diagnostics · Live Feed** — the event socket's query items. Columns for both
pipelines in one table. Client-side filters over the rows held. The panel states
that it starts empty, holds a bounded ring, and retains nothing — there is no
server-side query store to search. A cached marker is not a verdict.

- **This is the only page that subscribes to `query`.** It adds `query` on mount
  and drops it on unmount, per the protocol frozen in `p5-03`. Leaving the
  subscription open after navigating away would put the whole household's
  per-query feed back on a phone that is showing Settings — and would keep the
  engine doing per-query publish work for a page nobody is looking at.
- Dropping it is what closes the connection when the next route needs no events.
  Health and Memory need none; Settings holds `config_changed` only.
- **It is a Live Feed, not a Query Log.** The term matters: a log promises
  retained history and FAH keeps none. Use the vocabulary CONTEXT.md carries
  after this phase reconciles it.

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

Settings fields stack label-over-control. The memory KPI rail and composition bar
stay legible without pinch-zoom.

## Acceptance criteria

- A settings write sends only the changed keys — asserted by inspecting the
  request body in a test.
- The All settings panel shows keys the curated form does not model, and shows
  **no** `auth.*` material.
- `[api]` fields are either absent from the form or gated behind a consequence
  confirmation that names the lock-out.
- No bound, enum or mutability class in the form is sourced from the API
  response — they are hand-carried, and the review file says from where.
- The restart banner appears on a boot-only change and clears after a restart.
- Rotating the API key warns that the key is shown once and that existing
  clients break.
- The memory composition bar's segments sum to RSS; peak is not among them.
- No memory or latency budget is drawn as a wall. The watch and over-budget
  rules are dotted markers with captions, and the over-budget state never
  appears without the budget line that names it.
- The Live Feed ring is bounded, pauses when hidden, and never grows with
  uptime. At 390 px it matches `sketch/MobileLiveFeed.dc.html`: card per event,
  200-row ring, hidden-page pause stated on the page.
- The `query` subscription is added on entering the Live Feed and dropped on
  leaving it — asserted against a running API, by observing that no query frames
  arrive once another page is shown, **and** that the engine stops doing
  per-query publish work.
- **Route-scoped, per the phase invariant.** Settings holds `config_changed`
  while mounted and nothing else; Health and Memory hold no subscription, so the
  socket is closed while either is the active route. Leaving any of the four stops
  its traffic within one refresh interval.
- The restart-required banner is global UI state with **no** timer of its own: it
  revalidates on entering Settings, and opportunistically when a mounted page's
  shared refresh reads `/health`. It does not clear live from an unrelated page,
  and that is deliberate — a banner is not worth a standing poll.
- Degraded status is explained identically wherever it appears — one string in
  the code, not three.
- Correct in both themes at all three breakpoints, verified at 390 px.
- Gates green, cargo and frontend. Bundle size recorded, gzip and brotli.

## Out of scope

Any config key the API rejects on that endpoint — `rules.lists`, `policies` and,
after `p5-04`, `auth.*`. Editing from the raw panel.

## Suggested prompt

> Read docs/dashboard/information-architecture.md sections Settings and
> Diagnostics, docs/dashboard/open-questions.md section Closed, API.md,
> CONFIGURATION.md for the sections being exposed, and
> plan/open/phase5/p5-09-settings-and-diagnostics.md. Build the hand-written
> Settings form with its raw panel, changed-keys-only writes, hand-carried
> validation metadata and gated `[api]` fields, and the three Diagnostics views
> with the Live Feed owning the `query` subscription. Design the phone layout for
> each, including the Live Feed as cards.
