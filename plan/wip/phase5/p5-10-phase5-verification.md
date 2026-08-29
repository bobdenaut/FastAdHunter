# P5-10 — Phase 5 Verification

**Phase:** 5 · **Depends on:** p5-09 · **Model:** Opus

## Goal

Prove the dashboard on the real device against the real budgets, and record the
evidence. Nothing new is built here — this task measures, tests end to end, and
closes the phase.

## Context

Budgets that apply, from PERFORMANCE.md and this phase's own constraints:
container image at most 30 MB, measured at 13.0 MiB before this phase; RAM
steady-state at most 128 MB; the frontend bundle at most 150 KB gzip. Budgets
are compared against a pre-change checkout, never against a stored baseline —
see [docs/measurement-traps.md](../../../docs/measurement-traps.md).

## Execution split — decided 2026-08-29

The p2.6-11 L.3 soak runs on the production container until
**2026-09-01T07:57Z** plus its day-7 acceptance. Deploying a phase-5 image
restarts `fah-next` and voids the soak, so this task executes in two stages.
The scope below is unchanged; only its ordering is constrained.

| Stage | When | Scope items |
| ----- | ---- | ----------- |
| **A — dev box, now** | during the soak, no router contact | end-to-end tests; route-ordering regression; bundle measurement (gzip + brotli, chunks, uPlot, login path); image build + size + no-Node check; page-by-page trace; route-scoped fetching table (against a local `fah-api`); socket-load run (local); mobile pass over the LAN against the dev-box server |
| **B — on-device, after p2.6-11 closes** | after the day-7 acceptance is written, one router intervention | deploy; the three RSS readings; Argon2id cost on the device; polled-endpoint cost (`/health`, `/telemetry`, `/cache`); refresh-default correction in `constants.ts`; certificate re-confirmation on the shipped image |

Rules:

- **No deploy proposal before the p2.6-11 day-7 acceptance exists.** The
  Stage B deploy is proposed together with the p2.6 post-soak cleanup
  (old-container removal, comment swap) so the router is touched once, not
  twice. All commands are owner-run, per root CLAUDE.md.
- If Stage A completes first, the task sits **`AWAITING SOAK`** in the phase
  table, cell naming Stage B as what flips it.
- Stage A results are recorded in the task's review file as they land;
  Stage B appends to the same file. One review file, two dated sections.
- **RAM budget caveat for Stage B:** the ≤ 128 MB row is steady-state. The
  p2.6 audit ([phase2.6-audit.md](../../../docs/code-review/phase2.6/phase2.6-audit.md),
  F9) recorded transient peaks to 150.6 MiB on the soaking build from
  boot-compile/list-refresh, invisible at the 360 s sample cadence. Read the
  three RSS readings against steady-state, and record `process_peak_rss`
  separately — a peak above 128 MB is the known transient, not an automatic
  budget failure, and gets attributed (F3's dropped p2.5 criterion) rather
  than averaged away.

## Scope

- **End-to-end tests** against a running `fah-api`: sign in, load every page,
  perform one mutation per mutating page, sign out. A page that renders but
  whose data never arrives must fail the test, not pass it silently.
- **Route-ordering regression test** kept from p5-01, re-asserted: unknown API
  paths still return the API's JSON error, never the SPA shell.
- **Bundle measurement**: total size **in gzip and in brotli**, the chunk
  breakdown, the largest contributors, and the cost of uPlot stated separately.
  Gzip is the gate; brotli is what the handler actually serves, so both are
  recorded. Confirm the login path does not load the chart chunk and that no web
  font ships unless one was justified in `p5-05`.
- **Image measurement**: final image size against the 30 MB budget, and the
  delta this phase added. Confirm the runtime image contains no Node, no
  `node_modules`, no source maps, and no build toolchain.
- **RSS measurement on the RB5009**, as three readings, not one — a socket
  subscribed to `query` costs engine-side publish work that static serving does
  not, and reporting them together measures the wrong thing:
  1. baseline, no dashboard;
  2. dashboard served, browser on a page that does **not** subscribe to `query`;
  3. browser on the Live Feed.

  Static serving should be close to free between 1 and 2; if it is not, say by
  how much and why. The 2→3 delta is the event feed's own cost.
- **Auth cost on the device**: login latency and peak RSS during Argon2id
  verification, re-measured on the final build, with the rate limiter **and the
  concurrency semaphore** engaged, against the transient peak allowance `p5-04`
  recorded — not against the ≤ 128 MB steady-state row.
- **Socket load**: one browser on the Live Feed for a sustained period — event
  rate, client memory, and confirmation that the ring stays bounded and the
  engine sheds nothing it would not otherwise shed. Separately, a browser on the
  Dashboard for the same period must receive **no** `query` frames, and the
  engine must do no per-query publish work while no socket subscribes to them.
- **Route-scoped fetching, measured end to end.** The phase invariant is that an
  inactive page has approximately zero API activity attributable to it, and this
  is where that is proven rather than asserted per task. With a request log on one
  side and the server's connection count on the other:

  | Check | Expected |
  | ----- | -------- |
  | Sit on each of the thirteen screens for ten minutes | only that screen's own reads; no request it does not need |
  | Navigate away from each screen | its traffic reaches zero within one refresh interval |
  | Active route on any of the nine event-free screens | **WebSocket closed**, confirmed by server connection count |
  | Enter Live Feed, then leave | `query` subscribed then released; engine per-query work starts and stops with it |
  | Background the tab briefly, then return | polling stops at once; **no reconnect** — the grace period absorbs a normal app switch |
  | Background the tab past the grace period | socket closed; on return it is recreated with exactly the active route's subscriptions |
  | Visit all thirteen screens in sequence | client memory does not grow with the number visited |

- **Cost of the polled endpoints on the RB5009.** `GET /health`,
  `GET /api/v1/telemetry` and `GET /api/v1/cache`: median service time and CPU
  cost per call, with `/cache` measured at two cache occupancies to establish
  whether stage counting is proportional to entries.

  `p5-05` set the background-refresh defaults — `/health` 60 s, `/telemetry`
  300 s, `/cache` 300 s — and the option sets offered on the card selector
  (`/health` 30/60/300 s, `/telemetry` and `/cache` 60/300 s) from reasoning,
  not measurement. **They are provisional and are corrected here from the
  measured figures**, in `dashboard/frontend/src/constants.ts`. Whether
  `/telemetry` and `/cache` may offer a 30 s option is decided by this
  measurement and by nothing else.

- **Mobile pass**: every page opened on a real phone over the LAN, checked
  against the phone artboards in [docs/dashboard/sketch/](../../../docs/dashboard/sketch/)
  — `MobileNav`, `MobileDashboard`, `MobileLiveFeed`, `MobileClients` — which are
  the source of truth for these criteria:
  - no horizontal body scroll on any page at 390 px; wide content scrolls only
    inside its own container;
  - every interactive control at least 44 px on its smallest axis;
  - the sidebar is an overlay drawer below 768 px, dismissible by scrim and by
    close control;
  - no table-heavy view renders as a horizontal table — Lists, Clients and the
    Live Feed are one card per row;
  - the Live Feed holds its narrow-viewport ring and stops rendering while the
    page is hidden. Verify by backgrounding the tab and confirming no work
    continues.
- **Certificate experience re-confirmed on the final build.** `p5-02` already
  settled the SAN mechanism and measured browser behaviour; this task checks the
  shipped image still behaves that way and that the session cookie persists on
  the phone after a container restart. It does not re-open the decision.
- **A page-by-page trace**: for every figure rendered anywhere in the UI, the
  API field it comes from. Anything that cannot be traced is a defect, not a
  footnote.
- **Doc updates** — see the section below; each needs its own approval.
- All measurements go to `docs/code-review/phase5/` with corpus, workload and
  device, per root CLAUDE.md rule 19.

## Acceptance criteria

- Every sidebar entry resolves to a working page against a live API.
- Bundle at most 150 KB gzip, recorded — with the brotli figure and the chunk
  breakdown beside it.
- Image at most 30 MB, recorded, with the phase's delta stated.
- Runtime image proven free of Node and build tooling.
- Steady-state RSS on the device recorded as the three readings above.
- Argon2id parameters confirmed acceptable on the device under rate limiting and
  the concurrency bound, against the stated transient allowance.
- Live Feed proven bounded over a sustained run; a non-subscribing page proven to
  receive no `query` frames.
- Every rendered figure traced to an API field, and every derived figure —
  `permitted` is the only one — labelled as derived.
- Mobile pass completed on a real device, with the certificate behaviour
  re-confirmed against `p5-02`'s findings.
- Gates green, cargo and frontend, `request_coverage.rs` included.

## HTTPS and the certificate — settled in p5-02, confirmed here

The dashboard is served over the API's existing TLS listener and inherits
whatever certificate that listener presents. `p5-02` measured what household
browsers do with it, fixed the SAN set and defined the regeneration migration.

**This task confirms, it does not re-decide.** Re-run the phone check against the
shipped image and record any difference from `p5-02`'s findings. The deployment
recommendation lives in SECURITY.md and the deployment notes — not in the UI.

**Explicitly out of bounds:** an HTTP fallback to dodge the warning. The session
cookie is `Secure` and `__Host-`-prefixed, so it does not exist over plain HTTP;
serving the dashboard unencrypted would send a session credential and the whole
config surface across the LAN in clear. If the warning is unacceptable, the
answer is a trusted certificate, never a downgrade.

## Out of scope

New features. Anything the capability matrix marks as cut. Any change to the
dashboard intended to make a certificate warning less visible. Re-opening the
`p5-02` certificate decision.

## Doc updates

PERFORMANCE.md gains the bundle and image figures if they belong there;
`docs/project-state.md` is rewritten to say where the work now stands.
**Both are repository documents needing the owner's explicit yes for that
concrete change** — propose the edits, then wait.

## Suggested prompt

> Read PERFORMANCE.md section Budgets, docs/measurement-traps.md,
> docs/dashboard/capability-matrix.md, the p5-02 and p5-04 review files, and
> plan/open/phase5/p5-10-phase5-verification.md. Run the end-to-end tests,
> measure bundle in gzip and brotli, image, the three device RSS readings and
> login cost against a pre-change checkout, complete the mobile pass, trace every
> rendered figure to its API field, and write the evidence to
> docs/code-review/phase5/. Propose the PERFORMANCE.md and project-state.md edits
> and wait for approval.
