# Phase 5 — Whole-Phase Audit

**Scope:** the entire Phase 5 implementation as it stands on `phase5-09`
(working tree, `d736122` + uncommitted doc edit). Source inspection of
`dashboard/frontend/src` (all core infrastructure and every page), `fah-api`
auth/session/routes/web/events, the p5-01…p5-09 review files, plus a live
browser pass: the built bundle under `vite preview` against a local stub API
(full REST fixture set + hand-rolled WebSocket server), Chromium via
Playwright. **The RB5009 was not touched.** No repository code was modified;
the one reproduction test was created, run, and deleted.

Gates re-run independently, not read from the review file:
`npm run typecheck` clean · `npm run test` **923 passed / 54 files / 0
failed** · `npm run build` **128,363 B gzip (83.6 % of budget), brotli
113,765 B**, postbuild scan green · `cargo test --workspace` **1,196 passed /
0 failed**. All four match the p5-09 review's §10.5 figures exactly.

---

## BLOCKER / MUST FIX

### A1 · SocketManager strands in `backoff` when the union empties during a probe — later routes never reconnect

`src/events/socket.ts` — `runProbeCycle()` / `scheduleBackoff()` re-check
nothing after the `await this.probe()` resolves except `disposed` and
`sendsToLogin`. Sequence, **reproduced with a throwaway vitest against the
real classes** (both assertions below failed):

1. A route with events is active; the server refuses upgrades (3 immediate
   failures) → state `probing`, probe request in flight.
2. The operator navigates to a socketless route → union empties →
   `onUnionChanged` correctly tears down, resets counters, sets `closed`.
3. The probe resolves (`unreachable`/`inconclusive`/`session-valid`) →
   `scheduleBackoff()` runs **unconditionally**: state becomes `backoff`,
   detail line set, timer armed.
4. The backoff timer fires while the union is still empty → `connect()`
   early-returns on `union().length === 0` **leaving state `backoff`** with
   no timer armed.
5. The operator enters the Dashboard or Live Feed → `onUnionChanged` runs
   `connect()` only from `closed` (`socket.ts:199`) — state is `backoff`, so
   **no socket is ever opened again**.

Measured: after step 4 `indicator()` reads `reconnecting` (with
`server unreachable` detail) on pages that want no socket, and after step 5
zero new sockets are opened (`sockets.length` flat, state `backoff`). The
Dashboard then lives on its mount `GET /stats` for ever; the Live Feed stays
empty; the indicator lies. The only recoveries are a tab hide/show cycle
(`setSuspended(false)` does handle `backoff`) or a reload. No test in
`socket.test.ts` covers a union change during an in-flight probe.

This breaks the phase's own WebSocket-lifecycle invariant, silently and
semi-permanently. Fix shape (not prescribed): re-check the union when the
probe resolves and when the backoff timer's `connect()` declines, collapsing
to `closed` — the same reset `onUnionChanged` already performs.

---

## SHOULD FIX BEFORE P5-10

### B1 · Failed sign-out paths report success and overclaim revocation

- `shell/shell.tsx:99` `signOut`: `logout().catch(() => undefined).then(navigate)`
  — a `NetworkError` (request never reached the server) still lands on
  `/login` with no message; the session cookie remains valid in the browser.
- `settings/access-card.tsx:26` `signOutEverywhere`: `.finally(navigate)` and
  the comment claims "the secret is rotated server-side either way" — untrue
  for a request that never arrived. The `setError` before it is unmounted by
  the navigation and can never render. An operator who pressed **Sign out
  everywhere** on a flaky connection is told nothing while every session,
  their own included, stays valid.

Low likelihood on a LAN, but this is the one control whose entire purpose is
revocation; surfacing "the server was not reached — nothing was revoked"
before navigating costs a sentence.

---

## NON-BLOCKING

| # | Where | Finding |
| - | ----- | ------- |
| C1 | `shell/shell.tsx` `NOT_FOUND` + `pages/not-yet-built.tsx` | An unknown URL renders the not-yet-built copy — "is not built yet · this screen lands with its own task" (browser-verified at `/dev/gallery` in the prod bundle). Wrong story for a mistyped URL now that no route is unbuilt; the placeholder's only remaining consumer is the 404. |
| C2 | `pages/dashboard.tsx` `useHistory` vs `pages/performance/use-perf-history.ts` | Near-identical disambiguation hooks. The Dashboard half still lacks the single-flight `/config` join and the `adopt` ordering stamp that Performance and Settings gained (this is p5-06's deferred **F11**, verified still present at `dashboard.tsx:208–220` — now also one standard behind Settings' G3 guard). |
| C3 | `pages/settings.tsx` | A field's 422/validation error stays rendered while the operator retypes the field; it clears only on the next Save. Cosmetic — Save always revalidates. |
| C4 | `docs/code-review/phase5/p5-09-…-review.md` §8/§9.5/§10.5 | The open list still names **N24** ("no test drives `UpstreamServers` or `DestinationList`"), but `settings.test.tsx:569/601` now drives both (keystroke-with-caret, newline survival). Stale by one fix pass; **T4**'s narrower edges (hostname cell, multi-row, delete shift) genuinely remain untested. |

Deferred rows recorded in earlier reviews and spot-checked as still present,
unchanged in kind: p5-06 **F12** (two `GET /health` at boot — observed in this
audit's own request log), p5-09 **N21** (`/diagnostics/memory` fetches
`/telemetry` raw beside the registry), **M2**, **N5/N6/N8/N9/N12/N16/N22/N23**,
**T2/T3**. None re-litigated here; they stand as their files record them.

---

## VERIFIED SOUND

Explicit re-verification, not inherited from the review files. Evidence noted
per row.

**Security / auth (source, `fah-api`)**

- `auth.rs`: `PUBLIC_PATHS` is exactly `["/health", "/api/v1/auth/login"]`
  (test-pinned); everything under the auth layer takes bearer **or** session;
  `?token=` is accepted on `/api/v1/events` only (test-pinned), so the key
  never rides a REST URL.
- `session.rs`: token = version ‖ expiry ‖ 16-byte nonce, HMAC-SHA256 via
  `aws-lc-rs`; expiry enforced from the payload; unknown version rejected even
  with a valid MAC; secret never `Debug`/`Display`s; staged atomic writes with
  stray-tmp discard. Cookie is `__Host-fah_session` + `Secure; HttpOnly;
  SameSite=Strict; Path=/` (test-pinned).
- `routes.rs`: login is TLS-gated (503 without `api.tls`), behind a per-IP
  rate limit **and** an Argon2 concurrency permit; `no_store` on all four auth
  routes; a session-authenticated WS upgrade must pass `same_origin`
  (scheme + host + port, IPv6-aware); password change and logout-all both
  rotate the session secret, so "every session dies" is true in code, not just
  in the UI copy.
- `GET /config` carries no `auth` key at any layer; the raw panel strips
  defensively anyway; the rotated API key is rendered once and stored nowhere
  (`rotate-dialog.tsx`); no masked key is drawn.

**API coverage / contract / ownership**

- Every path the frontend calls exists in `routes.rs` with the right method;
  `api/types.ts` spot-checked against `wire.rs` (`StatsResponse`,
  `ClientResponse` incl. `skip_serializing_if` on `assignment_source`,
  `Memory` flatten, `DebugMemoryResponse`, `HealthResponse`) — all agree.
- No frontend caller of `GET /history/top` or `GET /clients/{ip}/policy` — the
  Clients page reads policy off `GET /clients` items as p5-03 intended.
- `PERF_FIELDS` and `MEMORY_PERF_FIELDS` are disjoint const lists;
  `max_points` is sent only by Memory (1440/5000/5000, all ≤ the server cap).

**Route-scoped fetching + WS lifecycle (live browser, stub API)**

- Dashboard entry traffic is exactly the declaration: the five polled
  endpoints' first fetch + `/stats`, `/config`, `/history/summary` one-shots +
  the shell's `/health` (twice — that is F12, above). Subscribe frame:
  `{"subscribe":["stats"]}`, byte-exact.
- Parked on Custom Rules for 8 s: **zero** API requests.
- Live-feed → Dashboard on **one** socket: `["query","stats"]` then
  `["stats"]` — acquire-before-release observed on the wire. Dashboard →
  Lists: `["stats","list_refreshed"]` then `["list_refreshed"]`. Every
  socketless route closed the connection; Settings opened
  `["config_changed"]` alone; Memory issued exactly `/debug/memory`,
  `/telemetry`, `/history/perf` and nothing polled.

**Refresh architecture (source + live)**

- One timer per subscribed endpoint, last-unsubscribe stops it and aborts the
  in-flight request; `invalidate` joins an in-flight fetch and restarts the
  timer only on success; suspend stops all timers and resume refetches only
  stale slots; `observe` is read at exactly its two documented sites; interval
  preferences validate against the offered list and propagate cross-tab.
- Save on Settings (live): one `POST /config` followed by exactly one
  `GET /config` re-read; dirty line named the single edited key.

**Live Feed (live + source)**

- Newest-first confirmed on the wire-driven page (`q94 … q91` at the top);
  pager clamps; **one tree** at both widths, and a mid-visit 1400→390 px
  resize swapped table→cards with zero horizontal overflow while the ring's
  capacity stayed at its mount value (G1 fix holds in a real browser).
  `BoundedRing.clear()` preserves `pushed`, so row keys never repeat.

**State, races, disposal**

- Settings: single-flight reader with `startedAfter` + `adopt` ordering stamp
  (G3) present; rebase keeps edits and drops server-matched ones.
- Recompiling mutations (rules PUT, policy create/lists-change/delete) block
  navigation, hold a modal, and carry no abort signal; popstate is replayed to
  the held path. Live mutations (clients, list toggles) abort-and-reread with
  per-row busy state and skip the re-read after unmount (`disposed` refs).
- Restart banner: judged at `EndpointState.fetchedAt`, pending announcements
  skipped (B3 fix intact); no clock in `services.ts`.

**Boundedness**

- Feed ring fixed at mount (500/200); rule-tester ring capped at 10; refresh
  registry retains at most the five endpoint slots; Lists' `pending` is
  bounded by configured lists; localStorage keys are a fixed five-entry set.

**Derivations / provenance**

- `derive.ts` is the one arithmetic module; the R*/E*/D* functions match
  their authorising tables; `stackedMemory` gaps over-accounted components
  while keeping RSS (G4 holds); `upstreamStateCounts` ignores unknown states;
  `latencyMs(0) → null` keeps no-traffic out of the charts; no client-side
  residual, no averages, no traffic-share inventions found anywhere in the
  page trees.

**Mobile / responsive (live, 390 × 844)**

- All twelve routes: `scrollWidth − clientWidth = 0` on both
  `documentElement` and `.main`. Feed renders one card per event. (Touch
  target sizes were not re-measured; p5-09 V6's measurements stand.)

**Prod hygiene / regression**

- `/dev/gallery` is not in the prod bundle (renders the 404 path — see C1 for
  the copy); postbuild forbidden-content scan green; bundle inside budget.
- The invariant suites are real enforcement, honest about being source pins:
  `timers.test.ts`, `filtering-invariants.test.ts`,
  `system-invariants.test.ts` (comment-stripped before asserting absences),
  `routes.test.ts`, `observe-callers.test.ts`. The p5-09 G-pass tests for B1
  and B2 assert caret and newline survival — behavioural, not greps.
- Zero console errors across the full navigation sweep, both themes; the
  `Diagnostics · Memory` group prefix renders; theme toggle applies
  `data-theme` correctly.

---

**Verdict: BLOCKED** — on **A1** alone. A2-level severity would be arguable
given the three-failure trigger, but the failure is silent, user-facing,
session-long, and sits in the exact mechanism Phase 5 is measured on; it needs
a fix and a regression test before p5-10's live verification, which would
otherwise certify a socket lifecycle with a known dead end in it. B1 is
strongly recommended alongside; C1–C4 and the standing deferred rows do not
block.

*Superseded by the fix pass below.*

---

## Fix pass — A1, B1, C1, C2, C3

Owner-requested scope: those five findings. C4 (a stale line in the p5-09
review's open list) was not touched. No endpoint, no new figure, no `.rs`
change, no deferred row reopened.

| # | Fix | Where |
| - | --- | ----- |
| **A1** | The manager may never hold a state but `closed` while the union is empty. `scheduleBackoff()` collapses to `closed` when the union emptied while the probe was in flight (the reproduced path), and `connect()`'s empty-union decline now settles `closed` instead of leaving the caller's state standing — both through one `settleClosed()`, which also drops the stale detail line so the indicator cannot read `reconnecting` on a route that wants no socket. | `events/socket.ts` |
| **B1** | Both sign-out paths navigate **only on an answer**. A `401` counts as one (the session is already gone; the shared guard bounces). A request that never reached the server keeps the operator where they are: the shell renders a `Sign out failed — you are still signed in` banner (cleared on navigation), and the Access card renders the error plus *"Nothing was revoked — every session, this one included, is still valid."* The comment claiming the secret was "rotated server-side either way" is gone. | `shell/shell.tsx`, `settings/access-card.tsx`, `styles/components.css` (`.signout-banner` margin, shared with `.restart-banner`) |
| **C1** | `pages/not-yet-built.tsx` deleted with its 404-only consumer role; `pages/not-found.tsx` replaces it — *"No screen at this address"*, with a `Link` home. The `built` flag's doc comment in `routes.ts` no longer names a component that does not exist. | `pages/not-found.tsx`, `shell/shell.tsx`, `router/routes.ts` |
| **C2** | The disambiguation protocol is one implementation: `pages/recorded.ts` exports `useConfigReader()` (the single-flight `/config` join) and `useRecordedRange()` (one request per range, the empty-answer `/config` re-read, the three failure paths). `usePerfHistory` is now a thin wrapper over it; the Dashboard's private `useHistory` is deleted and the page calls the shared pair — closing p5-06's **F11** (its second concurrent `/config`) as a side effect of the dedup rather than as a third copy of the guard. | `pages/recorded.ts` (new), `pages/dashboard.tsx`, `pages/performance.tsx`, `pages/performance/use-perf-history.ts` |
| **C3** | Editing a field deletes that field's entry from the error map — only that field's; a second field's rejection stands until its own edit or the next Save. | `pages/settings.tsx` |

### Tests — five added, **928** total

| Test | Asserts |
| ---- | ------- |
| socket · probe outlives the union | after the deferred probe resolves and all timers run: state `closed`, indicator `not-needed-here`, detail `null`, and the next `acquire` opens a new socket |
| settings · sign out everywhere, request never reached the server | stays on `/settings`, renders *Nothing was revoked* |
| settings · sign out everywhere, server answered | lands on the login page (green pre-fix too — the keep-working guard) |
| settings · field error clears on retype | the message is gone the moment the field is edited again |
| settings · the other field's error stands | two invalid fields, one retyped: exactly one message left, under the untouched field |

**Pre-fix proof.** The three fixed sources were stashed and the new tests
re-run against them: **4 failed** — the strand test on `expected
'reconnecting' to be 'not-needed-here'`, the sign-out failure case navigated
anyway, and both field-error cases kept the stale message. Stash popped; all
green.

### Browser re-check (stub API + `vite preview`, Chromium)

- `/nonexistent-page` renders *No screen at this address* with the Dashboard
  link — the not-built story is gone.
- The refactored Dashboard renders its full card set and issues exactly
  **one** `GET /config` on entry (mount read and any disambiguation share the
  joined reader).

### Gates

| Gate | Result |
| ---- | ------ |
| `npm run typecheck` | clean |
| `npm run test` | 54 files, **928 passed**, 0 failed |
| `npm run build` | **128,767 B gzip** — 83.8 % of the 153,600 B budget; brotli 114,138 B; postbuild scan passes |
| `git status -- crates/` | empty — no `.rs` changed |

Bundle moved 128,363 → 128,767 B gzip (**+404 B**) for the five fixes.

### Verdict

**PASS WITH DEFERRED FINDINGS.** A1, B1, C1, C2 and C3 closed; the blocker is
gone. Open and unchanged: **C4** (stale N24 row in the p5-09 review's open
list — a `.md` edit, left for the owner) and the standing deferred rows the
audit's NON-BLOCKING section lists (p5-06 F12 et al., p5-09 M2/N*/T*).
