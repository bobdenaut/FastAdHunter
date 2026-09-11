# p3-09 — Rejection View and Interception Document Editor — Review

**Task:** [p3-09-rejection-view-and-document-editor.md](../../../plan/wip/phase3/p3-09-rejection-view-and-document-editor.md) ·
**Plan:** [p3-09-rejection-view-and-document-editor-plan.md](../../../plan/wip/phase3/p3-09-rejection-view-and-document-editor-plan.md) ·
**ADR:** [ADR-0008](../../decisions/0008-live-interception-and-client-certificate-rejection.md)
step 3 · **Depends on:** p3-07 (endpoint + `details`), p3-08 (`status 525`) ·
**Base:** `phase3-06` at `f11aa53` · **Gates:** GREEN ·
**Review:** 2026-09-10, §Findings — fixes applied (§Outcomes), verdict
**PASS WITH DEFERRED FINDINGS**

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

- **The 390 px / both-themes check ran on 2026-09-11 and found three defects,
  all now fixed.** Performed against the p3-06 probe serving the real dashboard
  and API on the RB5009, driven by Playwright at 390 px and 1280 px in both
  themes. This supersedes the reuse argument recorded here before — "every class
  used is already carried by a surface verified at that width" — which was
  sound for the classes it named and blind to the three below, because each is
  a property of the *container*, not of the shared class.

  | # | Defect | Fix in `components.css` |
  | --- | --- | --- |
  | 1 | The line editor never grew past the textarea's intrinsic `cols=20` (161 px) at any width — `.editor-area` is already `width: 100%`, but the wrapper between it and `.set-field-control` is a flex item and shrinks to content. At 1280 px: 161 px used of 473. An IPv6 CIDR truncated behind a sideways scrollbar | `.set-field-control:has(.editor) > * { flex: 1 1 auto; min-width: 0 }`. `:has()` scopes it to editor rows; toggles and selects in the same container still size to content, verified unchanged at 240/320 px |
  | 2 | `Exclude` rendered a 34 px touch target while every other phone control took 44 px. The phone block names `.feed-actions .btn` and `.feed-chipset .chip`; this button is a `.btn` inside `.ev-meta`, which neither reaches. It is the only control in the view | `.feed-cards .ev .btn { min-height: 44px }` in the existing `max-width: 767px` block |
  | 3 | The focused textarea painted over the sticky save bar while a long editor scrolled, covering `Save changes`. `.editor-area` is `position: relative; z-index: 1`, `.set-bar` was `position: sticky; z-index: auto`, same stacking context — `auto` loses to `1`. Found by the owner scrolling, not by the sweep | `z-index: 2` on `.set-bar` |

  Defect 3 is **not** specific to this card: `.set-bar` is the shared settings
  save bar, so every section with an editor tall enough to scroll had it —
  `[egress]`'s `allow_destinations` most obviously. Defect 1 is likewise shared;
  it was measured on the Interception card and reproduced on `[egress]`.

  Verified after the fixes: editor 161 → 429 px at 1280 and 161 → 287 px at 390,
  no sideways scroll, full `2a02:2f04:5400:cc00::/64` and
  `mob-ro.unicreditbanking.eu` visible; `Exclude` 44 px, matching Pause; the save
  bar topmost under `elementFromPoint` at its own centre with the textarea
  focused. No horizontal page overflow at 390 px in either theme; contrast 13.29:1
  (domain), 6.63:1 (secondary), 15.04:1 (editor text). Frontend suite 1043 tests,
  58 files, green. Evidence and per-state figures in
  [p3-06-testing-results-2.md](p3-06-testing-results-2.md) §Session 3.

  One scope limit: the rejection-view rows were injected into the DOM using the
  component's own card markup, because the R7 steer was removed after the N-rows
  and no live rejections arrive. The check therefore covers the CSS and layout,
  not the data path. The Interception card was measured entirely as served.
- **Doc consequences written** (owner's explicit go, 2026-09-10):
  `docs/dashboard/information-architecture.md` §Diagnostics → Live Feed gains
  the rejection sub-view, and §Settings gains "The Interception card — a card,
  not a section". API.md and CONTEXT.md need nothing (plan §8).
- Two tabs editing the document is last-write-wins (ADR). The card rebases on
  the response, so the loser sees it on its next save.
- The telemetry counter `client_cert_rejections` is not consumed; no dashboard
  listener-counter type exists (plan F8).
- **The view cannot tell a pinning app from a client that never trusted the CA,
  and `Exclude` is the wrong remedy for the second.** Measured on device
  2026-09-11 ([p3-06-testing-results-2.md](p3-06-testing-results-2.md)
  §Session 3, N3): with the CA removed from an Android phone, the view filled
  with 56 rows — Spotify, Brave, Facebook, Heytap, Allawn — each offering the
  same single action. The p3-08 contract assumes such a client sends
  `UnknownCA` and so never reaches this view; Android sends a 525-class alert
  instead. The footer text ("the client refused the certificate we present for
  that host") stays literally true and is still misleading here, because an
  operator following the only action offered would permanently surrender
  interception for a host in order to work around a missing CA install. No
  change is proposed yet — the shape of the fix (a second action, a hint, or a
  classification change in p3-08) is a design decision. Scope: one device
  (OnePlus 15, OxygenOS, BoringSSL).

## Findings

Reviewer: Fable 5.1, 2026-09-10. Scope: `4400572` against `f11aa53` — 18
files, frontend plus one doc, no `.rs` touched — checked against the plan, the
task file and ADR-0008 §The operator's path / §detect ≠ auto-exclude. Every
line below was verified in the tree, in the server sources named, or by
re-running the gate, not against the summary above. No sub-agents, no review
workflow.

### Gates re-run by the reviewer

| Gate | Result |
| --- | --- |
| `npx vitest run` | 58 files, 1039 tests pass — matches the summary |
| `npm run build` | gzip 136 373 B (88.8 % of 153 600), brotli 120 971 B — matches the summary |
| `git diff --check f11aa53..HEAD` | clean |
| cargo gates | not re-run — the diff contains no `.rs` file |

### Contract parity verified (server ↔ client)

| Claim | Server | Client | Holds |
| --- | --- | --- | --- |
| `details` shapes, keys, 0-based `index` / `duplicate_of` | `fah-rules/src/interception.rs:547-566`, `fah-api/src/error.rs:202-247` | `documentErrorDetails` | yes |
| `normalizeHost` mirrors the matcher | `interception.rs:85-88` — `trim().trim_end_matches('.')`, ASCII lowercase | trim, `/\.+$/`, `toLowerCase()` | yes for ASCII, which SNI is |
| `isExcluded` is the `ExclusionSet::contains` walk | `interception.rs:70-82` — exact, then every parent suffix down to the TLD | same walk | yes |
| `ts` has a fixed number of digits | — | comment at `rejections.ts:66` | **no** — F-04 |
| No polling, no route change | — | `live-feed.test.tsx`: 60 s fake timer, request count stays 1, subscriptions unchanged; `routes.ts` absent from the diff | yes |
| Writes only after click + confirm; exact host only | — | `exclude()` reachable only from `ConfirmDialog.onConfirm`; `withExclusion` appends its argument verbatim | yes |

### Severity-ranked

Blockers at review time: **F-01** — one byte. Fixed; see §Outcomes.

**F-01 · Major · maintainability / tooling** —
[rejections.ts:47](../../../dashboard/frontend/src/pages/live-feed/rejections.ts#L47)

- Evidence (measured): the group key is `` `${row.client}<U+0000>${row.domain}` ``
  with a *raw* NUL byte in the source, not the `\0` escape. `git diff --stat`
  reports the file as `Bin 0 -> 4383 bytes`, `--numstat` gives `-	-`, `file`
  says `data`, and `rg -n groupRejections dashboard/frontend/src/pages/live-feed/`
  lists the test file only — ripgrep skips the defining file as binary, silently.
- Impact: every future diff, blame and PR view of this file is "Binary files
  differ", and every symbol search through the Grep tool misses it. For a repo
  whose review process is diff-and-grep, the module is invisible.
- Fix: the escape `\0` — or F-08, which removes the key. Before DONE.

**F-02 · Minor · concurrency, lost write** —
[rejections-view.tsx:292](../../../dashboard/frontend/src/pages/live-feed/rejections-view.tsx#L292),
`:50`

- Evidence: `busy` is `saving === key`, so while row A's read-append-`PUT` is
  in flight every other row's Exclude stays enabled. Row B's fresh `GET` can
  answer before A's `PUT` commits; both `PUT`s then carry the same base and the
  later one drops the other host. The view adopts whichever response lands
  last, so the dropped row reverts to a button with no notice.
- Impact: one confirmed exclusion lost. Window: one server round-trip after
  the first confirm — inference, not reproduced; the modal narrows it, does not
  close it.
- Fix: `disabled={saving !== null || !ready}`, label only the saving row. One
  line; before DONE.

**F-03 · Minor · state, fresh read discarded** —
[rejections-view.tsx:93-96](../../../dashboard/frontend/src/pages/live-feed/rejections-view.tsx#L93-L96),
[live-feed.test.tsx:983](../../../dashboard/frontend/src/pages/live-feed.test.tsx#L983)

- Evidence: `exclude()` reads `current`, builds the `PUT` from it, and adopts
  only the `PUT` response. On a `duplicate` rejection `document` is still the
  copy from mount, so the row prints "Already excluded" **beside a live Exclude
  button**; the test at line 983 codifies this ("the page's copy of the
  document is still the right one"). It is not: `duplicate` is the server
  proving the live document already covers the host. Same for a parent entry
  added from another tab — the `PUT` succeeds and appends a redundant child.
- Impact: a row offers an action that can only fail again or add noise; every
  further click repeats the `PUT`.
- Fix: `setDocument(current)` right after the `GET` — the pill then reads
  "excluded" through `isExcluded` — and update the test. Whether to also skip
  the `PUT` when `isExcluded(host, current.exclude_domains)` is the owner's
  call: plan §3.2 froze "no dedupe" for `withExclusion`, which this does not
  touch. Before DONE (the adopt).

**F-04 · Minor · correctness, ordering** —
[rejections.ts:66-71](../../../dashboard/frontend/src/pages/live-feed/rejections.ts#L66-L71)

- Evidence (measured against the formatter): the comparator orders groups by
  lexical `ts` on the premise "fixed number of digits". `ts` is written by
  `fah-api/src/timestamp.rs:16` through the `time` crate's RFC 3339 writer,
  which omits the fraction when the nanoseconds are zero and otherwise writes
  it *trimmed* (`time-0.3.55/src/formatting/formattable.rs:868-874`,
  `truncated_subsecond_from_nanos`); the repo's own expectation is
  `"1970-01-01T00:00:00Z"` (`fah-api/src/wire.rs:1224`). Lexically
  `…:00Z` > `…:00.5Z` and `…:00.5Z` > `…:00.55Z` — both backwards.
- Impact: groups whose newest events fall inside one second can be listed out
  of order. Cosmetic and sub-second, but the comment is false and the premise
  will be copied.
- Fix: compare `Date.parse(left.last) - Date.parse(right.last)` — per group
  per flush, not per row — or store the parsed instant on the group. Delete
  the comment. Before DONE.

**F-05 · Minor · silent failure, defensive** —
[interception-card.tsx:297-305](../../../dashboard/frontend/src/pages/settings/interception-card.tsx#L297-L305),
`:357-388`

- Evidence: an `invalid_entry` or `duplicate` whose `index` maps to no sent
  line (`sent[list][index]` undefined) yields two empty anchor lists and
  `message: null`; `rejection` stays `null`, `anchored` is false, nothing
  renders, and the bar still says "Unsaved changes". No producer of that skew
  exists today — server indexes are 0-based over the list as sent
  (`interception.rs:216`) — so this is defensive.
- Impact: a save that fails with no visible outcome, should the contract drift.
- Fix: when the built anchor list is empty, fall back to `cause.message` as
  the card-level line. Three lines; before DONE.

**F-06 · Minor · UX regression in non-HTTPS modes** —
[interception-card.tsx:219-233](../../../dashboard/frontend/src/pages/settings/interception-card.tsx#L219-L233)

- Evidence: `inert ? <mode note> : saved ? … : pristine ? … : 'Unsaved
  changes…'` — in `dns` and `dns+http` the mode note *replaces* the status
  line, so "Unsaved changes", "No unsaved changes" and "Applied on the next
  connection." never appear there. The card's own comment calls listing the
  first client before switching modes a legitimate order; that operator gets
  no unsaved indicator and no save confirmation.
- Fix: render the mode note as its own line and keep the status line. Before
  DONE.

**F-07 · Minor · load failure hides the groups** —
[rejections-view.tsx:112](../../../dashboard/frontend/src/pages/live-feed/rejections-view.tsx#L112)

- Evidence: `loadError` returns `ErrorState` alone; the groups derive from
  `rows` and need no document. No retry — the mount effect runs once; the
  operator must leave and re-enter the view.
- Impact: a document-store hiccup (`503`) blanks the detection surface the
  operator came for.
- Fix: keep rendering the groups with `ready=false` and place the `ErrorState`
  above. **Deferred** — `ErrorState` has no retry affordance anywhere in the
  dashboard; a wider change than this task.

**F-08 · Minor · duplication** —
[rejections.ts:47](../../../dashboard/frontend/src/pages/live-feed/rejections.ts#L47),
[rejections-view.tsx:254](../../../dashboard/frontend/src/pages/live-feed/rejections-view.tsx#L254)

- Evidence: one identity, two key schemes — client + NUL + host in
  `groupRejections`, client + space + host in `keyOf`.
- Fix: export one `keyOf` from `rejections.ts` (or carry `key` on
  `RejectionGroup`) and use it in both; F-01 goes with it. Before DONE.

**N-01 · Nitpick · per-render allocation** —
[rejections.ts:96](../../../dashboard/frontend/src/pages/live-feed/rejections.ts#L96)

- `isExcluded` rebuilds the normalized `Set` of the whole list for every group
  on every flush: O(groups × list) `trim` + regex + `toLowerCase` per frame.
  Realistic sizes (≤ 10 groups, tens of entries) are nothing; the worst case
  (500 × 512 = 256 k normalizations per flush, flushes frame-coalesced) is
  not. Inference, not measured. Memoise the normalized set on `document` and
  pass it in. **Deferred.**

**N-02 · Nitpick · shadowed global** —
[rejections-view.tsx:45](../../../dashboard/frontend/src/pages/live-feed/rejections-view.tsx#L45),
[interception-card.tsx:60](../../../dashboard/frontend/src/pages/settings/interception-card.tsx#L60)

- `const [document, …]` shadows the DOM `document` in two components, one of
  which holds `HTMLTextAreaElement` refs. TypeScript would catch a misuse;
  readability only. **Deferred.**

**N-03 · Nitpick · wording** —
[live-feed.tsx:166-172](../../../dashboard/frontend/src/pages/live-feed.tsx#L166-L172)

- The header card stays titled "Filters" in the rejection view while every
  filter is hidden; only the view switch and Pause / Clear remain.
  **Deferred.**

**N-04 · Nitpick · dead refs** —
[interception-card.tsx:73-74](../../../dashboard/frontend/src/pages/settings/interception-card.tsx#L73-L74)

- `clientsRef` / `excludeRef` exist only to satisfy `LineEditor`'s required
  `textareaRef`; nothing reads them. Making the prop optional touches a shared
  component. **Deferred.**

### Test coverage

- Covered: grouping, order across seconds, normalize and parent walk,
  exact-host body, cancel → no `PUT`, `duplicate` / `over_cap` / `503`
  rendering, pause, no polling, "pinned" absent, editor round trip, anchor
  mapping through blank lines, both duplicate lines, `over_cap` card line,
  `500` wording, mode note.
- Not covered: F-02 (two rows in flight), F-04 (two groups inside one
  second), the view's `invalid_entry` / `shape` branches, F-05's empty-anchor
  path.

### Docs checked

- `docs/dashboard/information-architecture.md` additions match the shipped
  behaviour — view chip, hidden filters, confirm naming the host,
  card-not-section, caps, `details`-only placement. Nothing to change.
- API.md / CONTEXT.md: no new shape or term — agreed with plan §8.
- The 390 px / both-themes check remains unperformed, as the summary states;
  stays deferred.

### Categories checked

| Category | Result |
| --- | --- |
| Ownership / lifetimes | n/a (TS); mount effects abort on unmount, `PUT` never aborted by design |
| API surface | `ApiError.details` optional-with-`null` keeps the ten call sites valid; `interception.ts` mirrors its sibling modules |
| Allocations / copies | N-01 only; `withExclusion` copies two small arrays |
| Duplication | F-08 |
| Concurrency | F-02, F-03; cross-tab last-write-wins accepted by the ADR |
| Error handling | F-05, F-07; `message` never parsed — verified by reading both mappers |
| Security | session-gated; the only write is the confirmed `PUT`; host rendered through JSX escaping; the body carries the server's own SNI string, validated server-side |
| Hot path | none touched — dashboard only |

### Outcomes

Fixes applied 2026-09-10 on the owner's go. F-03 as the adopt only — the
`PUT` is still sent; skipping it stays the owner's call.

| # | Outcome | Where |
| --- | --- | --- |
| F-01 | **fixed** — the raw NUL is gone; the key is `keyOf(row.client, row.domain)`. `file` reads `JavaScript source, UTF-8 text`; `rg -n "export function groupRejections" …/live-feed/` now finds `rejections.ts:50`. The diff against `4400572` still prints `Bin` because the *old* blob holds the byte; from the next commit on the file diffs as text | `rejections.ts:47` |
| F-02 | **fixed** — `ready = document !== null && saving === null`, passed to both `Action`s. Test "offers one exclusion at a time, every other row waiting on the write" holds a `PUT` open and asserts both rows disabled, then one pill and one enabled button once it lands | `rejections-view.tsx`, `live-feed.test.tsx` |
| F-03 | **fixed** (adopt) — `setDocument(current)` before the `PUT`. The duplicate test now models the fresh copy (first `GET` empty, second covering the host) and asserts the pill, no "Already excluded", no button, one `PUT` | `rejections-view.tsx`, `live-feed.test.tsx` |
| F-04 | **fixed** — parsed once per group (`instantOf`; unparseable → 0, sorts last), `right.at - left.at`; comment corrected. Unit test with `…:00Z`, `…:00.25Z`, `…:00.5Z` inside one second | `rejections.ts`, `rejections.test.ts` |
| F-05 | **fixed** — `placed()` returns the bands when a sent line answers, else the card-level line in the server's words (`verbatim`); `stated()` / `Outcome` fold the four one-line returns. Test "states a rejection it cannot place rather than showing nothing" (`index: 7` over one entry) | `interception-card.tsx`, `interception-card.test.tsx` |
| F-06 | **fixed** — the mode note is its own `<p class="note">` above the bar; the status line always renders. The inert test also asserts "No unsaved changes." then "Unsaved changes" after an edit, note still present | `interception-card.tsx`, `interception-card.test.tsx` |
| F-08 | **fixed** — one `keyOf(client, host)` exported from `rejections.ts`; the view's copy deleted | `rejections.ts`, `rejections-view.tsx` |
| F-07 | **fixed** (second cycle, on the owner's direction) — the failed read renders above the groups, not instead of them: `ErrorState`, a line saying the rows are the ring's and excluding needs the document, and a Retry button that bumps an `attempt` counter the mount effect depends on. Buttons stay held back through `ready` until a read succeeds. Test "keeps the groups when the document cannot be read, and retries on request": first `GET` 503 → one row listed, Exclude disabled; Retry → second `GET` 200 → Exclude enabled, two requests total | `rejections-view.tsx`, `live-feed.test.tsx` |
| N-01 | **fixed** — `exclusionsOf(list): ReadonlySet<string>` normalizes once; `isExcluded(host, exclusions)` takes the set. The view memoises it on `stored`; the unit tests build it once per case | `rejections.ts`, `rejections-view.tsx`, `rejections.test.ts` |
| N-02 | **fixed** — the state is `stored` / `setStored` in both components; `buffersOf(stored)`, `withExclusion(current, host)`; the `.then` parameters that shadowed the state are `response` | `rejections-view.tsx`, `interception-card.tsx`, `rejections.ts` |
| N-03 | **fixed** — the header card is titled "View" in the rejection view, "Filters" otherwise | `live-feed.tsx` |
| N-04 | **fixed** — `LineEditor.textareaRef` is optional, spread onto the `<textarea>` only when given (`exactOptionalPropertyTypes` forbids an explicit `undefined` against Preact's `ref?: Ref<T>`); the card's two refs and its `useRef` import are gone; `rules.tsx` still passes its ref | `line-editor.tsx`, `interception-card.tsx` |
| 390 px / both-themes | **open** — left open by the owner, 2026-09-10; Known limitations stands | — |
| — | housekeeping, owner's ask: root `.gitignore` ignores `node_modules/` at any depth; a stray `./node_modules/.vite/vitest/` cache left by a vitest run started at the repo root was deleted | `.gitignore` |

Two fix cycles: the first fixed the "before DONE" set and carried the
reviewer's own "deferred" marks forward as if approved; the owner directed the
rest. A reviewer's defer is a recommendation — the decision is the owner's.

Gates after the fixes, both cycles:

| Gate | Result |
| --- | --- |
| `npm run typecheck` (inside `npm run build`) | clean |
| `npx vitest run` | 58 files, 1043 tests pass — +4 over the summary (F-02, F-04, F-05, F-07); the F-03 test replaced in place |
| `npm run build` | gzip 136 538 B (88.9 % of 153 600), brotli 121 149 B — +165 B over the summary's figure |
| `git diff --check` | clean |
| cargo gates | not re-run — no `.rs` file changed |

### Verdict

**PASS WITH DEFERRED FINDINGS** — every finding, F-01 … F-08 and N-01 … N-04,
fixed in the working tree with gates green. One item open, by the owner's
instruction: the 390 px / both-themes check (Known limitations).
