# Phase 5 — Web Dashboard

**Objective:** the dashboard — a static, API-only web interface served by
`fah-api` itself. Pi-hole supplies the visual and interaction language; the FAH
API decides what exists. Every screen is backed by a route that ships or by one
this phase adds deliberately in `p5-03` — nothing is stubbed, mocked, or invented
to fill a Pi-hole-shaped hole.

**ROADMAP.md lists Phase 5** — see its §Phase 5 — Web Dashboard entry in
[ROADMAP.md](../../../ROADMAP.md). The owner committed the phase on 2026-08-25;
the roadmap was updated to match in `ef76d59`, so the mandate is one this file
can point at. The design record is [docs/dashboard/](../../../docs/dashboard/):
[capability-matrix.md](../../../docs/dashboard/capability-matrix.md) is the
source of truth for what is built,
[information-architecture.md](../../../docs/dashboard/information-architecture.md)
for where it goes, and
[visual-system.md](../../../docs/dashboard/visual-system.md) for how it looks.

**Why this order:**

```text
static serving → certificate evidence → API contracts → auth
    → typed frontend foundation → pages → verification
```

Static serving first — it depends on no decision still open, settles the
deployment shape end to end, and puts a binary on the device the later work is
measured against. The certificate spike second, because the whole session design
rests on a browser accepting this box's certificate on a real phone, and the
generated SANs do not currently cover the address the dashboard is opened at
(`crates/fah-api/src/tls.rs`); discovering that after auth is built is the wrong
end of the phase. API contracts third — the `/events` subscription protocol and
the `GET /clients` policy fields are backend changes the typed client is written
against, so they are frozen in one task rather than discovered page by page.
Auth fourth, because every page behind it needs the session contract fixed before
a line of frontend code assumes one. Foundation fifth — shell, typed client,
socket manager and the size gate — so every page after it is a page and not also
a framework decision. Pages then land in dependency order: the Dashboard proves
the shell, the charts and the socket together; the filtering pages share one
editing idiom; the runtime pages share one charting idiom; Settings and
Diagnostics come last because they are the most opinionated and benefit from
everything learned. Verification closes it against the budgets.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p5-01-static-serving.md` | `fah-api` serves `/web`; multi-stage image; route ordering; boot check | Opus | DONE |
| 2 | `p5-02-cert-browser-spike.md` | Real desktop/phone evidence; SAN decision and code change; regeneration migration | Opus | DONE |
| 3 | `p5-03-api-contracts.md` | `/events` subscription protocol, `GET /clients` policy fields, reserved API docs | Opus | DONE |
| 4 | `p5-04-auth-session.md` | Argon2id password, session cookie, login/logout, cookie on REST + WS (heavy) | Opus | DONE |
| 5 | `p5-05-frontend-foundation.md` | Vite/TS/Preact shell, typed client, socket manager, size gate, login page | Opus | DONE |
| 6 | `p5-06-dashboard-and-lists.md` | Dashboard and Lists — proves tiles, charts, tables, mutations | Opus | DONE |
| 7 | `p5-07-filtering-pages.md` | Custom Rules, Policies, Clients, Rule Tester | Opus | WAITING |
| 8 | `p5-08-runtime-pages.md` | Cache, Performance, Upstreams | Opus | WAITING |
| 9 | `p5-09-settings-and-diagnostics.md` | Settings (curated + raw), Health, Memory, Live Feed | Opus | WAITING |
| 10 | `p5-10-phase5-verification.md` | Bundle and image budgets, RB5009 validation, e2e against a live API | Opus | WAITING |

## Phase ordering — decided 2026-08-25

| Decision | Record |
| -------- | ------ |
| Phase 5 does **not** precede `phase2.6-adaptive-stage1` | 2.6 is the active `wip` phase with the L.3 soak in flight. Phase 5 does not displace it. **Superseded in part on 2026-08-26** — see §Parallel track: implementation runs alongside the soak, but 2.6 keeps `wip` and closes first. |
| Phase 5 **is** promoted ahead of `phase3` and `phase4` | Owner decision. The dashboard does not wait on HTTPS interception or HTML filtering. |
| Phase 3 and Phase 4 each trigger a follow-up dashboard review | Not a redesign now, and no Phase 5 screen is built around a speculative feature — but the surfaces below are known to move. |

Surfaces to re-review after Phase 3 lands: certificate UI (Phase 3 adds generate
CA / import PEM-PFX / export CA / status, which supersedes whatever `p5-02`
settles) and per-client HTTPS-interception controls on **Clients**.

Surface to re-review after Phase 4 lands: the **Lists** rule partition. Cosmetic
rules (`##`, `#@#`) are counted in `rules_inactive` today (API.md §GET /lists);
activating them splits a fourth band out of it, exactly as `rules_active_url` was
split out in p2-03. The stacked bar and its tooltip change.

**The phase-selection algorithm is unchanged.** `plan/CLAUDE.md` picks the
lowest-numbered phase in `open`, which is `phase3`. The promotion above is a
decision recorded here, not in the algorithm — the owner performs the
`open` → `wip` move (root CLAUDE.md §Working agreement 3), so the two cannot
silently disagree without a human in between.

## Parallel track — decided 2026-08-26

Phase 5 implementation runs **in parallel** with `phase2.6-adaptive-stage1`
while its L.3 soak completes. The soak runs a deployed artifact on the RB5009
and is independent of this repository's branch topology; the exact soaking
commit is tagged `soak-p2.6-11` (`1c430aa`, 0.2.20).

| Rule | Value |
| ---- | ----- |
| Phase directory | stays in `plan/open/phase5` — no `open` → `wip` move |
| `wip` | `phase2.6-adaptive-stage1` keeps it, alone, and closes first |
| Branch chain | `phase5-NN`, cumulative: `phase5-01` from `main`, `phase5-02` from the completed `phase5-01`, and so on |
| Merge to `main` | only when the phase is complete, and only the tip branch — it already carries every earlier task's history |
| Rebase | only when `main` moves under the stack; `rebase.updateRefs = true` is set so the intermediate branch refs follow |
| P2.6 isolation | Phase 5 commits touch `dashboard/` and `fah-api`. A 2.6 soak fix branches from `main`, never from the chain |

Selection is unaffected — the paragraph above still holds, and the owner names
each Phase 5 task explicitly.

## Quality gates for this phase

The workspace gates in [plan/CLAUDE.md](../../CLAUDE.md) still apply in full and
are still the ones that decide `DONE`:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

From `p5-05` on, a task that touches `dashboard/frontend/` must **also** pass,
from that directory:

```sh
npm run typecheck     # tsc --noEmit, no implicit any
npm run build         # must succeed, and must fail over the size budget
```

The bundle budget is **150 KB gzip for the whole application** and it is
enforced by the build, not audited by hand — a build that exceeds it exits
non-zero. Treat a budget failure exactly like a clippy failure: fix it, or stop
and report. **Brotli is reported alongside gzip** on every task that records a
size: gzip stays the gate because it is the stricter bound, but `.br` is what
actually travels, so the served figure is never left unmeasured.

`cargo test --workspace` includes `crates/fah-api/tests/request_coverage.rs`,
which scrapes `router()` and asserts every route it serves has a request file
under `requests/`. **A task that adds a route adds its fixture in the same
change**, or adds an entry to that test's `UNCOVERED` allowlist with the reason.
Adding an entry is a decision; forgetting a file is a red gate.

Node is a **build-time** dependency only. If a change would put Node, npm or
`node_modules` in the runtime image, it is wrong.

## Standing constraints for every task in this phase

1. **The API is the source of truth.** No screen without a route. No field
   invented, no figure derived that the API does not support, no placeholder
   data in a shipped build. If a page looks thin, the answer is a thinner page.
2. **`/web` is image content, never a volume.** The UI is versioned and deployed
   atomically with `fah-api`; the volume set stays `/config` and `/data`. A
   mounted `/web` would pair a rolled-back binary with a newer UI.
3. **No Pi-hole strings ship.** Not in markup, comments, alt text, page titles
   or asset names. The attribution lives in
   [visual-system.md](../../../docs/dashboard/visual-system.md) and nowhere in
   the bundle.
4. **CONTEXT.md vocabulary in the UI**: `pass` / `allow` / `block`, policy not
   group, compiled rules not "domains on lists", endpoint where the index is
   meant.
5. **`allow` and `permitted` are different words for different things.** `allow`
   is the explicit allow-verdict counter the API exposes
   (`counters.dns.allow`, `history/perf.allowed_delta`) — an exception match, a
   four-figure number against six-figure traffic. `permitted` is the derived
   band `queries − blocked`, which the API does not carry and the UI computes for
   charts. Never label a derived `queries − blocked` figure "allowed"; never
   relabel a real `allow` counter "permitted".
6. **Budgets are drawn as markers, never as walls.** Nothing enforces memory or
   latency at runtime — the container runs `memory-high=unlimited`. A chart that
   implies a hard limit invites someone to set one.
7. **Bounded everything, on the client too.** The event feed holds a fixed ring;
   no page accumulates state with uptime.
8. **The sketch is not source.** [docs/dashboard/sketch/](../../../docs/dashboard/sketch/)
   settles layout and interaction. Do not import from it, and do not treat its
   figures as measurements.

## Performance requirements — first-class, every task

Not aspirations. A task that trades one of these for UI convenience is wrong and
gets sent back.

| Requirement | What it rules out |
| ----------- | ----------------- |
| Static frontend only | no SSR, no Node runtime, no second container, no new port |
| Minimal dependencies | every addition earns its bytes against `visual-system.md` |
| Minimal initial JS | route-level lazy loading; **3–4 meaningful chunks**, not one and not dozens |
| No bundled web font | system font stack unless a named visual requirement justifies the 15–40 KB |
| Report gzip **and** brotli | gzip gates, brotli is what ships |
| No unnecessary polling | nothing polls what `WS /events` already pushes |
| Shared bounded refresh | one mechanism for `/telemetry`, `/cache`, `/health` — not a timer per widget — paused while the page is hidden |
| `/events` subscription filtering | a page that does not render queries does not receive them |
| Route-scoped data fetching | see below — the invariant this phase is measured against |
| No unbounded client buffers | fixed rings, listeners and timers torn down with their page |
| `/web` baked into the image | never a volume; the UI and the API version and deploy as one artifact |
| Existing budgets preserved | image ≤ 30 MB (13.0 MiB today), RAM ≤ 128 MB steady-state (46.6–53.6 MiB today) |

## Route-scoped data fetching — the phase invariant

**An inactive page has approximately zero API activity attributable to it.** That
is the objective, and it is the sentence every page task is measured against.

A page fetches and polls only what the **currently active route** renders.

1. **Leaving a page stops its work.** Route-specific timers are cleared and
   route-specific event subscriptions removed on unmount. An inactive page does
   not poll, does not fetch, and holds no subscription.
2. **Re-entering may reuse in-memory data, but must revalidate** according to that
   page's own refresh policy. Cached page data is bounded and does not accumulate
   across a session — thirteen visited pages must not mean thirteen retained
   payloads (hard rule 4 applies to the client too).
3. **Global state is allowed; global polling is not.** Shared state, a shared
   cache and a shared *mechanism* are fine. A timer that runs regardless of what
   is mounted is not.
4. **The shared bounded refresh is that mechanism, and it is route-scoped.**
   `p5-05` builds one reader for `/telemetry`, `/cache` and `/health`, so ten
   widgets wanting the same response cost one request. It is **not** a background
   poller: it polls an endpoint only while at least one mounted page subscribes to
   it, and the last unsubscribe stops that timer. Shared, not global — the two are
   easy to confuse and the distinction is the whole point.
5. **One `/events` connection, route-scoped subscriptions.** The socket may be
   shared, but it carries only the event types the active route needs. A page adds
   its types on mount and removes them on unmount. **If no active route needs
   events, the connection is closed**, not held idle.
6. **Hidden document: stop route polling and close the socket.** Recreate it and
   restore subscriptions on becoming visible.

   **Close on a grace period, not on the `visibilitychange` edge.** That event
   fires on every app switch, screen lock and notification pull — closing
   instantly costs a TLS handshake per glance at a phone. Pause rendering and
   polling immediately; close the socket after a short grace period still hidden.

   **Do not implement idling as an empty subscription.** `{"subscribe":[]}` looks
   like the cheaper option and breaks the server: `events.rs` sizes `SEND_TIMEOUT`
   around the 2 s stats cadence *because that traffic is what detects a peer that
   vanished without closing* — "a phone leaving Wi-Fi", which is the normal
   dashboard client. Remove the traffic and the watchdog can never fire, and a
   dead socket holds one of 64 connection slots until TCP gives up. Close it or
   keep it fed; do not leave it silent.

### Two consequences, decided rather than discovered

**The connection indicator has three states, not two.** Nine of the thirteen
screens need no events, so a closed socket is the correct steady state on most of
the UI and "disconnected" would be a false alarm on pages that never wanted a
connection. The indicator reads **live** · **not needed here** · **reconnecting**,
and only the third is a problem being reported. visual-system.md's `aria-live`
requirement stands, on those three states.

**The restart-required banner is global state without a global poll.** It
survives navigation, but nothing polls `/health` on its behalf from an unrelated
page: it revalidates on entering Settings, and opportunistically whenever a
mounted page's shared refresh reads `/health`. It will not clear live while the
operator is on the Cache page, and that is the accepted trade — a banner is not
worth a standing timer.

## Repository documents this phase will change

Listed up front so no task discovers one mid-flight. **Listing is not
permission.** Root CLAUDE.md §Working agreement 1 stands unchanged for Phase 5:
when a task reaches one of these files it proposes that concrete edit and waits
for the owner's yes. Nothing here is pre-approved.

| Document | Task | Change |
| -------- | ---- | ------ |
| API.md | p5-01 | static routes, stated unauthenticated |
| ARCHITECTURE.md | p5-01 | `fah-api` serves the UI |
| SECURITY.md | p5-01 | static surface exempt from the API key |
| SECURITY.md | p5-02 | certificate recommendation for a household deployment |
| API.md | p5-03 | `/events` subscription contract; `GET /clients` policy fields; reserved auth section |
| API.md | p5-04 | promote the reserved auth section to live; `auth.*` rules on `/config` |
| SECURITY.md | p5-04 | session model and its trade-off |
| CONFIGURATION.md | p5-04 | `[auth]` section and its mutability class |
| CONTEXT.md | p5-04 or p5-09 | `permitted` defined; Query Log reconciled with Live Feed |
| root CLAUDE.md | p5-05 | the layout note — `dashboard/` is no longer empty |
| PERFORMANCE.md | p5-10 | bundle and image figures, if they belong there |
| docs/project-state.md | p5-10 | rewritten to say where the work stands |
| ROADMAP.md | before the phase opens | Phase 5 as a committed phase; it is currently under "Backlog (no phase committed)" |

## TASK START / PHASE CONTEXT

Before starting a task:

1. Read the current task file completely.
2. Read the current phase status/table.
3. Read the **Implementation Summary** from the code-review files of previously completed tasks in the same phase that are relevant to the current task.
4. If the current task declares an explicit dependency (`Depends on: pY-XX`), always read that dependency's Implementation Summary.
5. Read full code-review findings only when the current task depends on a finding, deferred item, constraint, or decision that is not fully captured by the Implementation Summary.
6. Read any explicitly referenced architecture, security, API, configuration, or known-debt documents.

Do not read unrelated completed tasks or full review files merely because they belong to the same phase.

Do not re-litigate decisions already settled by previous tasks or reviews unless new evidence directly conflicts with them.

## TASK COMPLETION / REVIEW HANDOFF

When a task implementation is complete:

1. Do not summarize or describe the implementation in the chat.
2. Do not list changed files, implementation details, design decisions, benchmarks, tests, or findings in the chat.
3. Create the required code-review file immediately:
   `docs/code-review/phase5/<task-name>-review.md`
4. At the beginning of that review file, include a concise **Implementation Summary** describing:
   - what was implemented;
   - the relevant files/modules changed;
   - important design decisions;
   - tests/benchmarks run, if any;
   - any known limitations or deferred items.
5. The Implementation Summary may be based on the implementation and test results, but do not perform or document code-review findings yet.
6. Then stop. Do not perform the code review yet.
7. The only chat response after completing the task should be:

   `Task done. Report written to docs/code-review/phase5/<task-name>-review.md. Awaiting "start code review".`

8. Do not start the code review, add findings, or modify the findings section until the user explicitly says:
   `start code review`

When `start code review` is received, perform the CODE REVIEW procedure defined below and update the same review file.

**Do not implement, modify, revert, refactor, or otherwise change any code, configuration, tests, documentation, or architecture findings identified during the review without the user's explicit approval.**

The review phase is analysis and reporting only. After the review, stop and wait for explicit instructions before applying any fixes.

## CODE REVIEW

Every task must have a corresponding `*-review.md` file.
The file must be saved under `docs/code-review/phase5/`.

Before marking a task `DONE`, review the implementation as a senior reviewer
with standards comparable to Servo/Tokio review. For Rust, focus on:

- ownership, borrowing, and lifetime correctness
- API design and public interfaces
- unnecessary allocations and copies
- Rust best practices
- performance where relevant
- duplicated code or duplicated logic
- functions or logic that should be consolidated
- long-term maintainability
- concurrency and synchronization correctness where relevant
- error handling and failure modes where relevant
- security implications where relevant

For frontend code, focus additionally on:

- whether every rendered figure traces to an API field, and nothing is derived
  that the API does not support
- bundle weight: a dependency added, a polyfill pulled in, an asset inlined
- unbounded client state — rings, caches, listeners, timers that outlive a page
- error and empty states: a `422` anchored to its field, a `409` naming its
- conflict, an empty range rendered as "no data" rather than as a failure
- accessibility where it is cheap and permanent: focus order, contrast, colour
  never carrying meaning alone
- request discipline: no polling where the socket already pushes, no whole-
  document writes where the API takes a partial update

Ignore formatting, naming, and purely stylistic preferences unless they affect correctness, performance, maintainability, or API quality.

Do not propose architectural rewrites, new frameworks, or additional abstractions unless there is a clear, measurable technical benefit. Prefer minimal, targeted improvements over broad refactors.

Prioritize findings by severity:

- Critical
- Major
- Minor
- Nitpick

For every finding:

1. explain the technical rationale;
2. explain the impact if left unchanged;
3. state whether it should be fixed before the current task is marked `DONE` or explicitly deferred;
4. distinguish measured evidence from inference or recommendation.

A review is not complete until:

- the findings are recorded in the task's review file;
- addressed findings are verified;
- deferred findings are explicitly documented;
- the review concludes with a clear status: `PASS`, `PASS WITH DEFERRED FINDINGS`, or `BLOCKED`.

The review must not manufacture problems merely to produce findings. A clean review with no findings is valid.

**DO NOT PRESENT THE FINDINGS IN CHAT** - the user will read the review file!

## APPROVED FIXES / REVIEW FOLLOW-UP

When the user explicitly approves fixes from the code review:

1. Implement only the approved fixes.
2. Update the same code-review file with:
   - fixes applied;
   - verification results;
   - updated finding status.
3. Run the required gates.
4. Do not summarize or describe the fixes in the chat.
5. The only chat response after applying approved fixes should be:

   `Fixes applied. Review updated: docs/code-review/phase5/<task-name>-review.md. Gates green.`

6. Then stop and wait for further instructions.

Do not proactively report individual fixes, changed files, test counts, implementation details, or review findings in chat after an approved-fix cycle. That information belongs in the review file.

**Definition of done:** a browser on the LAN opens `https://<host>:8443/`, is
asked for a password, and lands on a dashboard that reads only from the FAH API.
Every sidebar entry resolves to a working page; every figure on every page
traces to a documented field. Lists, rules, policies, clients, cache and config
can be changed from the UI and the change survives a restart. The bundle is
under 150 KB gzip, the runtime image holds no Node, the image stays inside the
30 MB budget, and the whole thing is served by one binary on one origin with no
second container and no new port.

**Key risks:**

- **Argon2id on the shared runtime.** One multi-thread Tokio runtime serves DNS,
  HTTP and the API (`crates/fastadhunter/src/main.rs:242`). A verification run on
  a worker thread stalls DNS answering on a 4-core box. It goes through
  `spawn_blocking`, following the precedent already at `main.rs:757` (p5-04).
- **Argon2id memory on a 1 GB box shared with RouterOS.** Peak RSS is driven by
  **concurrent** verifications, not by their rate: a rate limiter alone does not
  bound it. A small semaphore with `try_acquire` does, and rate limiting stays
  as a separate control (p5-04).
- **Certificate SANs do not cover the LAN address.** `tls.rs` generates
  `fastadhunter`, `localhost`, `127.0.0.1`. The `__Host-`/`Secure` cookie design
  assumes browsers accept this origin on a real phone. Settled by evidence before
  auth is built, not recorded as a footnote afterwards (p5-02).
- **CSRF on the cookie-authenticated WebSocket.** Browsers send cookies on the
  upgrade and a WebSocket has no same-origin policy. `Origin` is validated
  against the request's own effective origin — no configured allowlist (p5-04).
- **Auth material in `GET /config`.** `routes.rs` returns the whole config and
  its comment asserts nothing there is secret. Adding `[auth]` breaks that, and
  the raw All-settings panel would render the password hash (p5-04, p5-09).
- **The event firehose.** Every connected socket receives every query event
  today. A phone parked on Settings pays for it, and the engine pays for the
  publish work. Fixed by the subscription filter — including the engine-side
  gate, not only the send path (p5-03).
- **Image budget.** ≤ 30 MB, measured at 13.0 MiB today. A multi-stage build
  keeps the toolchain out, but assets, source maps and fonts all land in the
  final layer if nobody is watching (p5-01, p5-10).
- **Bundle budget.** 150 KB gzip is generous for this design and trivial to
  blow with one convenience dependency. The gate is in the build for that
  reason (p5-05).
- **Route ordering.** A catch-all SPA fallback that shadows `/api/v1/*` turns
  every API 404 into an HTML page and every client error into a parse failure.
  Ordering is asserted by test, not by inspection (p5-01).
- **The curated Settings form drifting from the config.** A hand-written form is
  a subset by construction; the raw All-settings panel is what keeps an
  unexposed key visible rather than silently absent (p5-09).
