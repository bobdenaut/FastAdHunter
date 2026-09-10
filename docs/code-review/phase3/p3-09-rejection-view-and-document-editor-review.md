# p3-09 — Rejection View and Interception Document Editor — Review

**Task:** [p3-09-rejection-view-and-document-editor.md](../../../plan/wip/phase3/p3-09-rejection-view-and-document-editor.md) ·
**Plan:** [p3-09-rejection-view-and-document-editor-plan.md](../../../plan/wip/phase3/p3-09-rejection-view-and-document-editor-plan.md) ·
**ADR:** [ADR-0008](../../decisions/0008-live-interception-and-client-certificate-rejection.md)
step 3 · **Depends on:** p3-07 (endpoint + `details`), p3-08 (`status 525`) ·
**Base:** `phase3-06` at `f11aa53` · **Gates:** GREEN ·
**Review:** not started — awaiting `start code review`

## Implementation Summary

Two dashboard surfaces, both writing only through a whole-document
`PUT /api/v1/interception`. Frontend only — no crate, no route, no socket event
type, no CSS rule added.

| Surface | Where | What |
| --- | --- | --- |
| Rejection view | Live Feed sub-view (`view: 'rows' \| 'rejections'`) | `https` rows with `status 525`, grouped by client and host, count + last seen; one action per row — exclude the exact host, behind a `ConfirmDialog` naming it |
| Editor | Settings card, anchor `#set-section-interception` | `clients` and `exclude_domains` as two `LineEditor`s, one `GET` on mount, one `PUT` on save, server errors placed from `details` |

### What was implemented

| # | File | Change |
| --- | --- | --- |
| 1 | `src/api/core.ts` | `ErrorEnvelope.error.details?: unknown`; `ApiError.details` (5th constructor parameter, defaulted `null`); `envelopeOf` returns it |
| 2 | `src/api/types.ts` | `InterceptionDocument` |
| 3 | `src/api/interception.ts` (new) | `INTERCEPTION_PATH`, `getInterception`, `putInterception`, `DocumentList`, `DocumentErrorDetails`, `documentErrorDetails` |
| 4 | `src/api/index.ts` | re-exports |
| 5 | `src/pages/live-feed/rejections.ts` (new) | `REJECTED_STATUS`, `isRejection`, `RejectionGroup`, `groupRejections`, `normalizeHost`, `isExcluded`, `withExclusion` — pure |
| 6 | `src/pages/live-feed/rejections-view.tsx` (new) | table + narrow cards, confirm, exclude flow, error placement |
| 7 | `src/pages/live-feed.tsx` | `view` state, third chip row, routes the Feed card body, hides the filters and the rows footnote in the rejection view |
| 8 | `src/pages/settings/interception-card.tsx` (new) | the editor card |
| 9 | `src/pages/settings.tsx` | mounts the card, adds the `interception` nav anchor |
| 10 | `src/time.ts` | `eventClock(ts)` — the feed's private `time()` moved here, now shared with the rejection view |
| 11 | `docs/dashboard/information-architecture.md` | §Diagnostics → Live Feed: the rejection sub-view; §Settings: "The Interception card — a card, not a section" |

Tests: `src/api/interception.test.ts`, `src/pages/live-feed/rejections.test.ts`,
`src/pages/settings/interception-card.test.tsx` (new);
`src/api/core.test.ts`, `src/pages/live-feed.test.tsx`,
`src/pages/settings.test.tsx` (additions).

### Decisions

- **No CSS added.** Both surfaces reuse `table.t`, `.feed-scroll`,
  `.feed-cards`/`.ev*`, `.set-field*`, `.set-bar`, `.page-buttons`,
  `.pill.neutral` and `.editor*`. The `.feed-table` widths are tuned to ten
  columns, so the five-column rejection table uses the base `table.t` inside
  the same scroller. The "excluded" marker is `.pill.neutral`; the count column
  is `.num`.
- **`ApiError.details` is optional with a `null` default**, not a required
  parameter. Ten existing four-argument construction sites — production and
  tests — stay valid, and an envelope without the key reads `null` rather than
  `undefined`.
- **The exclude action re-reads before it writes.** `getInterception` →
  `withExclusion` → `putInterception`, so another tab's entries are not
  clobbered by a copy loaded when the view opened. Two `GET`s and one `PUT` per
  confirmed click is the documented request log.
- **The editor maps `index` → line through the entries it sent.** Blank lines
  are dropped on the way out, so `index + 1` would anchor one line off per blank
  line above the offending entry. `entriesOf` carries the source line.
- **An anchored rejection also gets a card-level sentence.** Bands alone left
  the operator to infer whether anything was written; the document is validated
  and swapped as one unit, so "nothing was stored" is stated, not implied.
- **`excludeMessage` (view) and `rejectionOf` (card) were not merged.** They
  consume the same four `reason`s but produce different artefacts — the view
  never anchors, the card never renders `invalid_entry`/`duplicate` as prose.
  The overlap is two sentences; a shared mapper would return a union both sides
  would have to destructure.

### Constraints held

| Constraint | How |
| --- | --- |
| No `restart_required`, no restart banner | asserted on both surfaces; the card never calls `armRestartBanner` and `restartArming()` is `null` after a save |
| No timer, no polling | fake-timer assertion: 60 s after entering the view, request count and socket-subscription count are unchanged; `routes.ts` untouched (`/live-feed` still `endpoints: []`) |
| Detection never mutates policy | a `PUT` happens only after click **and** confirm; cancel issues none |
| Only the exact host | `withExclusion` appends verbatim and never widens; no widening control exists |
| `message` never parsed | only `details.reason` is branched on; `message` is rendered verbatim where shown |
| Word "pinned" | asserted absent from the rendered view |
| Second buffer / history | none — the groups are derived from the ring snapshot the page already holds and discarded |

### Measurements

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --all-features --workspace` | pass |
| `npm run typecheck` | pass |
| `npx vitest run` | 58 files, 1039 tests pass (996 before — +43) |
| `npm run build` | pass, under budget |

| Bundle (gzip) | Before | After | Δ |
| --- | --- | --- | --- |
| Total | 132 701 B | 136 373 B | +3 672 B (+2.8 %) |
| Budget 153 600 B | 86.4 % | 88.8 % | +2.4 pp |
| Brotli | 117 707 B | 120 971 B | +3 264 B |

Before figure measured on a clean `phase3-06` @ `f11aa53` working tree
(`git stash push -u -- dashboard/frontend/src`), not on a stored baseline.

### Deviations from the plan

| # | Plan said | Shipped | Why |
| --- | --- | --- | --- |
| 1 | §3.4 "placed with the `[https]` section of the Settings form, anchored `#interception`" | after the form, before `AccessCard`, anchored `#set-section-interception` with a nav entry | there is no `[https]` section in `settings/metadata.ts` — the dashboard `Config` type carries no `https` key at all. The anchor prefix is the page's, so `#interception` alone would not be recognised by `readAnchor` |
| 2 | §4 "`settings.test.tsx` unchanged plus one mount assertion" | its `fetch` stub gained an `/api/v1/interception` arm and a read counter | the stub throws on an unrecognised URL, so the mounted card broke every test in the file |
| 3 | §3.3 "Filters do not apply in this view" | the verdict/pipeline/text controls and the `Filter…` toggle are not rendered in the rejection view | leaving them on screen offers controls that change nothing |
| 4 | — | `src/time.ts` gains `eventClock`; `live-feed.tsx`'s private `time()` deleted | the rejection view needs the same formatting; a second copy would be the duplication principle 4 forbids |

### Known limitations / deferred

- **The 390 px / both-themes check has not been performed on screen.** It needs
  the dashboard served against a running API (the dev proxy targets
  `https://localhost:8443`), which was not stood up. What is established
  instead: no CSS rule was added, and every class used is already carried by a
  surface verified at that width — `.feed-scroll`/`.feed-cards` switch in the
  `max-width: 767px` block, `.set-field` collapses to one column there, and
  `.editor` is the rules page's. This is a reuse argument, not a measurement.
- **Doc consequences written** (owner's explicit go, 2026-09-10):
  `docs/dashboard/information-architecture.md` §Diagnostics → Live Feed gains
  the rejection sub-view, and §Settings gains "The Interception card — a card,
  not a section". API.md and CONTEXT.md need nothing (plan §8).
- Two tabs editing the document is last-write-wins (ADR). The card rebases on
  the response, so the loser sees it on its next save.
- The telemetry counter `client_cert_rejections` is not consumed; no dashboard
  listener-counter type exists (plan F8).

## Findings

Not started. Awaiting `start code review`.
