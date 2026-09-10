# P3-09 — Rejection View and Interception Document Editor — Implementation Plan

**Task:** [p3-09-rejection-view-and-document-editor.md](p3-09-rejection-view-and-document-editor.md) ·
**ADR:** [ADR-0008](../../../docs/decisions/0008-live-interception-and-client-certificate-rejection.md)
frozen at `fcc7244` · **Depends on:** p3-07 (`GET`/`PUT /api/v1/interception`
and the `details` contract), p3-08 (`status 525` events) ·
**Status:** plan revised 2026-09-10 after owner review and the final
pre-implementation gate (F4, F8 folded in, §14); decisions frozen (§12);
awaiting implementation approval. Nothing implemented.

## 1. Objective and scope

Two dashboard surfaces, both writing only through a whole-document
`PUT /api/v1/interception`:

- **The rejection view** — a Live Feed sub-view listing `https` events with
  status 525 grouped by client and host, with one action per row: exclude the
  exact host observed, behind an explicit confirmation naming that host.
- **The editor** — a Settings card for the Interception Document: `clients`
  and `exclude_domains` as one-entry-per-line editors, saved as one `PUT`,
  with server validation errors anchored from the structured `details`.

No widening, no auto-exclusion, no bulk action, no `restart_required`
anywhere, no timer, no second buffer, no new route, never the word "pinned",
and no parsing of `message` strings.

## 2. Existing dashboard architecture (verified)

| Piece | Where | Fact used |
| --- | --- | --- |
| Stack | `dashboard/frontend` — Preact 10 + hooks, Vite, TypeScript, vitest (jsdom for pages); runtime deps `preact`, `uplot` only | no UI library; hand-written components |
| API client | `src/api/core.ts` `request<T>(path, opts)`, `ErrorEnvelope { error: { code, message } }`, `ApiError { code, status, retryAfter }`, `NetworkError`; modules per resource, re-exported from `api/index.ts` | `details` is added to the envelope type and the class (§3.1) |
| Types | `src/api/types.ts` — `QueryEvent { kind, ts, client, client_name, domain, …, status: number \| null, … }` (line 233), `Config`; `Telemetry` (line 166) carries `counters: Counters` only — **no `ListenerCounters` type exists in the dashboard** (F8) | a 525 arrives as a `query` item with `kind: 'https'`, `status: 525`, `domain` = SNI host; p3-08's `client_cert_rejections` counter is not consumed here |
| Events | `src/events/socket.ts`, `src/events/types.ts` `EVENT_TYPES = ['query','stats','config_changed','list_refreshed']`, `src/services.ts` | no new socket event type |
| Routes | `src/router/routes.ts` — each `Route` declares `events` and `endpoints`; `/live-feed` is the only `query` subscriber, `endpoints: []`, `ownsHeader: true`; `/settings` route | a sub-view of `/live-feed` inherits the subscription; a Settings card needs no route change |
| Live Feed | `src/pages/live-feed.tsx` (468 lines): `FeedBuffer` ring (500 desktop / 200 narrow), filters `verdict`/`kind`/`client`/`domain` (`filters.ts`, `KINDS = ['dns','http','https-sni','https']`), pause, paging, desktop table + narrow cards, `Detail`/`FeedVerdict`/`FeedCache` cells | the rejection view reads the same `rows` snapshot; pause semantics apply |
| Settings | `src/pages/settings.tsx` — hand-written sections (`settings/section-card.tsx`, `field-row.tsx`, `metadata.ts`, `patch.ts`), `raw-panel.tsx`, restart banner armed by `restart_required`; `config_changed` refreshes the form | the page composes cards in JSX; **`settings/access-card.tsx` is an existing card with its own endpoints and dialogs (`rotate-dialog`, `password-dialog`)** — proof the architecture supports a self-fetching card; no route needed |
| Whole-document editor precedent | `src/pages/rules.tsx` + `components/line-editor.tsx` (`LineEditor { value, onInput, anchors: EditorAnchor[] {line, message}, disabled, textareaRef, label }`), `PUT /rules/user`, 422 handling, `blockNavigation()`, pristine detection | reused; the anchor source changes from parsed text to `details` |
| Dialogs | `components/confirm-dialog.tsx` (`title`, `confirmLabel`, `cancelLabel`, `onConfirm`, `onCancel`, children), `focus-trap` | the exclude action's confirmation |
| Tests | `*.test.tsx` with `// @vitest-environment jsdom`, `render` + `act`, socket events pushed through `socket` (`live-feed.test.tsx`), `fetch` stubbed and read off (`settings.test.tsx`), fake timers | same patterns |
| Budget | `scripts/postbuild.mjs` `BUDGET_BYTES = 150 * 1024` gzip; `npm run build` fails over it | record before/after |
| No interception surface exists today | grep `interception`/`exclude_domains` in `src`: none | both surfaces are new |

## 3. Design

### 3.1 API module and the structured error

`src/api/core.ts`: `ErrorEnvelope.error.details?: unknown`; `ApiError` gains
`readonly details: unknown` (constructor parameter, `null` when absent);
`request()` passes `envelope.error.details ?? null`. Every existing throw site
is unaffected (absent → `null`).

`src/api/interception.ts`:

```text
INTERCEPTION_PATH = '/api/v1/interception'
getInterception(signal?) → Promise<InterceptionDocument>          // GET
putInterception(doc)     → Promise<InterceptionDocument>          // PUT, returns the stored document

type DocumentList = 'clients' | 'exclude_domains'
type DocumentErrorDetails =
  | { reason: 'shape' }
  | { reason: 'over_cap'; list: DocumentList; len: number; cap: number }
  | { reason: 'invalid_entry'; list: DocumentList; index: number; entry: string }
  | { reason: 'duplicate'; list: DocumentList; index: number; entry: string; duplicate_of: number }
documentErrorDetails(err: unknown): DocumentErrorDetails | null   // ApiError with status 422 and a details object of one of the shapes above; a runtime shape check, no string parsing
```

`src/api/types.ts`: `InterceptionDocument { clients: string[]; exclude_domains: string[] }`.
Nothing for the telemetry counter: the dashboard has no listener-counter type
and no surface that would show it (F8).

Error statuses this module's callers handle, from p3-07 §6: 400
`bad_request` (a body the server could not read as JSON — cannot be produced
by this client, handled by rendering `message` whole), 422 with `details`
(§3.3, §3.4), 503 and 500 (`message` whole). Only `details.reason` is ever
branched on.

### 3.2 Pure helpers — `src/pages/live-feed/rejections.ts`

```text
isRejection(row: QueryEvent): boolean          // kind === 'https' && status === 525
interface RejectionGroup { client: string; clientName: string | null; host: string; count: number; last: string }
groupRejections(rows: readonly QueryEvent[]): RejectionGroup[]   // key client+host, newest `last` first
normalizeHost(s: string): string               // trim, strip trailing dot, lowercase — mirrors the server's compile
isExcluded(host: string, list: readonly string[]): boolean       // exact or parent-suffix after normalizeHost — the ExclusionSet walk
withExclusion(doc: InterceptionDocument, host: string): InterceptionDocument  // append the host verbatim; no dedupe (the server answers `duplicate`)
```

Pure, no DOM, unit-tested. The grouping walks the ring snapshot the page
already holds — no second buffer, no history.

### 3.3 The rejection view — `src/pages/live-feed/rejections-view.tsx`

- Entered from a third chip row on the Live Feed header: `view: 'rows' | 'rejections'`,
  label **"Certificate rejected by client"**. Filters do not apply in this
  view; pause does (the snapshot is frozen); paging is hidden.
- On entering: one `getInterception()` so rows already covered by the document
  read "excluded" instead of offering the button. Re-read only from this
  page's own successful `PUT` response. No interval, no `refresh`
  registration — `/live-feed` keeps `endpoints: []`.
- Table (desktop): Last seen · Client · Host · Count · Action. Narrow: one
  card per group, as the feed's narrow layout does.
- Row action: **Exclude** → `ConfirmDialog` titled with the exact host
  ("Exclude `api.bank.example` from interception for every listed client? Its
  parent is not excluded; edit the document to widen.") → confirm →
  `getInterception()` → `withExclusion` → `putInterception` → on success the
  group shows "excluded". On `ApiError`: `documentErrorDetails` → `duplicate`
  renders "already excluded" and marks the row; `over_cap` renders the cap
  line (`len`/`cap`); `invalid_entry`/`shape` render the message (cannot
  happen for a host the server itself emitted, but handled); 503 renders the
  message; network → `ErrorState`. `blockNavigation()` while the `PUT` is in
  flight.
- Empty state: "No client has rejected our certificate since this page
  opened" — the ring starts empty (feed semantics), and the line says so.
- Wording fixed by the ADR: "Certificate rejected by client", "excluded",
  never "pinned".

### 3.4 The editor — `src/pages/settings/interception-card.tsx`

- A section card **Interception** placed with the `[https]` section of the
  Settings form, anchored `#interception`. Modelled on `access-card.tsx`: its
  own fetch, its own state, no participation in `patch.ts`, `metadata.ts`,
  the restart banner or `config_changed`.
- On mount: one `getInterception()`.
- Two `LineEditor`s: "Intercepted clients — one IP address or CIDR block per
  line" and "Never intercepted hosts — one hostname per line", each with a
  count against its cap (`n / 256`, `n / 512`, hand-carried constants — IA
  §Settings).
- Save → build the document from the buffers, dropping blank lines and
  keeping an `index → line` map per list → `putInterception(doc)` → rebase on
  the response. Reset restores the last loaded document. Pristine detection as
  `rules.tsx`.
- Errors from `documentErrorDetails`: `invalid_entry` / `duplicate` → an
  `EditorAnchor` at `map[details.list][details.index]` with `message`
  (`duplicate` also anchors `duplicate_of`); `over_cap` → the card-level line
  naming `list`, `len`, `cap`; `shape` → the message whole (cannot be produced
  by this card's own body; defensive); 503 → the message whole; 500 → the
  message whole with a "nothing was applied" note taken from the contract.
- After a successful save: "Applied on the next connection." No restart
  banner; the card never calls `armRestartBanner`.
- Mode note: when the Settings baseline's `engine.mode` has no HTTPS listener,
  the card states that the document is stored and takes effect at the first
  boot of a mode that has one.

### 3.5 Mobile and themes

Both surfaces use the existing card/table CSS and the feed's narrow
breakpoint. Verified at 390 px in both themes per the p5 convention.

## 4. File-by-file changes

| File | Change | Tests |
| --- | --- | --- |
| `src/api/core.ts` | `details` on the envelope type and `ApiError`; `request()` passes it | `core.test.ts`: with and without `details` |
| `src/api/types.ts` | `InterceptionDocument` only (F8) | type-checked |
| `src/api/interception.ts` (new), `src/api/index.ts` | §3.1 | `api/interception.test.ts`: paths, methods, body, `documentErrorDetails` accepts the four shapes and rejects others |
| `src/pages/live-feed/rejections.ts` (new) | pure helpers §3.2 | `rejections.test.ts` |
| `src/pages/live-feed/rejections-view.tsx` (new) | table/cards, confirm, action, error rendering | `live-feed.test.tsx` additions §6.2 |
| `src/pages/live-feed.tsx` | `view` state + chip row; render `RejectionsView` when selected; hide paging in that view | same |
| `src/pages/settings/interception-card.tsx` (new) | §3.4 | `settings/interception-card.test.tsx` §6.3 |
| `src/pages/settings.tsx` | mount the card; anchor entry | `settings.test.tsx` unchanged plus one mount assertion |
| `src/styles/*` | minimal: the "excluded" marker, count column | visual check |
| `docs/dashboard/information-architecture.md` | the two surfaces (doc consequence, not edited here) | — |

No change to `router/routes.ts`, `events/types.ts`, `patch.ts`,
`metadata.ts`, or the raw panel.

## 5. Data and control flow

```text
socket 'query' → FeedBuffer ring → rows snapshot
   rows view:        applyFilters → table (unchanged)
   rejections view:  groupRejections(rows) → groups; isExcluded(group.host, doc.exclude_domains) → button or "excluded"
       Exclude click → ConfirmDialog → getInterception → withExclusion → putInterception → response doc → local doc state
                                                                        ↘ ApiError → documentErrorDetails → row message

Settings → InterceptionCard mount → getInterception → two buffers (+ index→line maps on save)
   Save → putInterception(whole document from buffers) → response doc → rebase
        ↘ ApiError → documentErrorDetails → anchors / card line / message
```

Nothing on either surface writes except on a click; nothing polls; nothing is
parsed out of `message`.

## 6. Test strategy (vitest)

### 6.1 Unit — `rejections.test.ts`, `api/interception.test.ts`, `core.test.ts`

Grouping over mixed kinds/statuses (only `https` + 525 counted), count and
`last` per client+host, newest first, `client_name` carried; `normalizeHost`
trailing dot/case; `isExcluded` exact and parent only; `withExclusion` keeps
order and spelling; request paths/methods/bodies; `documentErrorDetails`
returns the typed shape for each `reason` and `null` for a 422 without
`details`, a non-422, a non-`ApiError`; `ApiError.details` round-trips through
`request()`.

### 6.2 Page — `live-feed.test.tsx` additions (jsdom)

- Push three 525 events for `client A / host X`, one for `client B / host X`,
  one `https` 200 and one `https-sni` block → the view shows two groups with
  counts 3 and 1 and nothing else.
- Exclude on the first row → confirm → `fetch` log: exactly `GET
  /api/v1/interception` then `PUT /api/v1/interception` with body
  `{ clients: <as returned>, exclude_domains: [<as returned>, 'X'] }` — the
  whole document plus exactly the observed host.
- Cancel in the dialog → no `PUT`.
- `PUT` answers 422 `{ details: { reason: 'duplicate', … } }` → "already
  excluded" renders, no second `PUT`, document state unchanged; `over_cap` →
  the cap line with `len`/`cap`; 503 → message.
- A host already in the returned `exclude_domains` (or under a parent entry)
  renders "excluded" with no button.
- Pause freezes the groups; resume refreshes them.
- Fake timers: advancing 60 s after entering the view produces no further
  `fetch` (no polling) and the socket subscription set is unchanged.
- The string "pinned" does not appear in the rendered view.

### 6.3 Card — `settings/interception-card.test.tsx`

- Mount → exactly one `GET /api/v1/interception`; the two editors show the
  lists one per line; counts `n / 256`, `n / 512`.
- Edit both → Save → one `PUT` whose body equals the displayed lines (blank
  lines dropped); response rebases; pristine again; the words "restart" and
  "restart_required" do not appear.
- 422 `{ details: { reason: 'invalid_entry', list: 'exclude_domains', index: 2, entry: … } }`
  with a blank line above the entry → the anchor lands on the correct editor
  line (index→line map); `duplicate` anchors both lines; `over_cap` renders the
  card line; document unchanged; Save stays enabled.
- 503 → message whole; 500 → message plus "nothing was applied".
- `engine.mode` without HTTPS → the stored-and-inert note is present; with
  HTTPS → absent.
- `config_changed` does not refetch the document (one `GET` total).

### 6.4 Gates

`npm run typecheck`, `npx vitest run`, `npm run build` (must stay under 150 KB
gzip; record before/after sizes in the review file), cargo gates unchanged.
Manual: both themes, 390 px, exclude flow against a dev binary running p3-07 +
p3-08.

## 7. Runtime, concurrency, performance, memory, security

- **Concurrency:** two tabs editing → last-write-wins (ADR); the card's
  rebase-on-response makes the winner visible on the next save of the loser
  (`duplicate` or a replaced list). No ETag in this task. A `PUT` in flight
  blocks navigation (`blockNavigation()`), so a route change cannot orphan a
  pending write's UI state; the server commit completes regardless (p3-07).
- **Cancellation:** `AbortSignal` on the mount `GET` only (as every page); the
  `PUT` is never aborted from the UI.
- **Performance:** grouping is O(rows) over ≤ 500 rows on each snapshot in the
  rejections view only; memoised on `rows` like `visible`.
- **Memory:** no new buffer; groups are derived and discarded; the document is
  one small object.
- **Security:** both surfaces run behind the session; the only writes are
  `PUT`s the operator confirmed; the view can only ever add the host it shows.

## 8. Documentation consequences (not edited here)

- `docs/dashboard/information-architecture.md`: Live Feed gains the rejection
  view (wording, one confirmed action, no widening); Settings gains the
  Interception card and its "not a config key, no restart banner" rule.
- API.md: nothing beyond p3-07/p3-08 (the `details` contract is p3-07's).
- CONTEXT.md: none expected.

## 9. Acceptance mapping

| Task / ADR line | Proof |
| --- | --- |
| Rejected host appears once per client with a count; exclude issues one `PUT` with the previous list plus exactly the observed host | §6.2 request-body assertion; manual end-to-end: next connection splices |
| The button cannot exclude anything but the exact observed host | `withExclusion` unit test; no widening control exists |
| The editor saves what it displays; a rejected `PUT` leaves the page's document unchanged and shows the server's error from `details` | §6.3 |
| No `restart_required` anywhere; no timer added | §6.2 fake-timer assertion; §6.3 string assertions; `routes.ts` diff empty |
| ADR 12 · nothing writes the document from observation (cross-task, p3-09 half) | §6.2: a `PUT` happens only after click + confirm; cancel → no `PUT` |
| Themes and breakpoints, verified at 390 px | manual, recorded in the review file |
| Gates green, cargo and frontend; bundle size recorded | §6.4 |

## 10. Out of scope and follow-ups

Widening to a parent (public-suffix list declined — ADR); auto-exclusion; any
`UnknownCA` or missing-CA surface; editing other `[https]` keys (they stay on
the config form); an ETag/version on the document; a socket event for document
changes; a persistent rejection history (the ring is the only store, by
design); a dedicated route for either surface.

## 11. Dependencies and order

After p3-07 (endpoint and `details` contract — the anchor logic is written
against §3.1's shapes, which are p3-07's `DocumentError` serialisation) and
p3-08 (525 events, counter). The pure helpers and the card can be developed
against a `fetch` stub before p3-08 lands; the manual end-to-end check needs
both.

## 12. Decisions — frozen by owner review, 2026-09-10

| # | Decision | Frozen as |
| --- | --- | --- |
| 7a | Exclusion granularity | exact host only |
| 7b | Confirmation | explicit `ConfirmDialog` naming the exact host before every `PUT` from the view |
| 7c | Editor placement | a Settings card (`access-card.tsx` proves the architecture supports a self-fetching card); no new route |
| 7d | Reuse | `LineEditor`, `ConfirmDialog`, `blockNavigation`, the `rules.tsx` PUT flow |
| 5 | Error consumption | `details` only (§3.1); `message` displayed, never parsed |
| — | Rejection view placement | Live Feed sub-view over the existing ring; no second buffer |
| — | Ring-bound visibility | rows older than the ring are gone; the empty state says "since this page opened" |
| — | Telemetry counter | not consumed; no dashboard type for it exists (F8) |

## 14. Final gate corrections (2026-09-10)

| # | Finding | Where fixed |
| --- | --- | --- |
| F4 (p3-07) | `PUT` gains a 400 `bad_request` envelope case | §3.1 error-status list |
| F8 | plan named `ListenerCounters` in `types.ts`; no such type exists | header, §2 Types row, §3.1, §4 row, §12 |

## 13. Final verification pass (owner's checklist, p3-09 half)

| Check | Where it holds |
| --- | --- |
| Acceptance: ADR line 12's p3-09 half covered once; task criteria mapped | §9 |
| No scope expansion | two surfaces, one API module, one envelope field consumed; no route, no socket event, no other `[https]` key |
| Concurrency / cancellation concrete | §7 |
| No hot path | dashboard only |
| `fah-api` / `fah-http` | untouched |
| Detection cannot mutate policy; no auto-exclusion | the view writes only after click + confirm (§6.2); no timer, no automatic `PUT` |
| Bootstrap with `clients=[]` | the editor can list the first client and the card says "applied on the next connection"; relies on p3-07 |
| Migration | not touched |
