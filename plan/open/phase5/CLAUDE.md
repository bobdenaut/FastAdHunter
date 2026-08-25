# Phase 5 — Web Dashboard

**Objective:** ROADMAP.md's dashboard deliverable: a static, API-only web
interface served by `fah-api` itself. Pi-hole supplies the visual and
interaction language; the FAH API decides what exists. Every screen is backed by
a route that already ships — nothing is stubbed, mocked, or invented to fill a
Pi-hole-shaped hole. The design record is [docs/dashboard/](../../../docs/dashboard/):
[capability-matrix.md](../../../docs/dashboard/capability-matrix.md) is the
source of truth for what is built,
[information-architecture.md](../../../docs/dashboard/information-architecture.md)
for where it goes, and
[visual-system.md](../../../docs/dashboard/visual-system.md) for how it looks.

**Why this order:** static serving first — it is the only stage that depends on
no decision still open, it settles the deployment shape end to end, and it puts
a binary on the device that the auth work can then be measured against. Auth
second, because every page behind it needs the session contract fixed before a
line of frontend code assumes one; building the shell against a bearer key and
migrating later would mean rewriting the client, the socket and the login flow.
Foundation third — shell, typed client, socket manager and the size gate — so
every page after it is a page and not also a framework decision. Pages then land
in dependency order: the Dashboard proves the shell, the charts and the socket
together; the filtering pages share one editing idiom; the runtime pages share
one charting idiom; Settings and Diagnostics come last because they are the most
opinionated and benefit from everything learned. Verification closes it against
the budgets.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p5-01-static-serving.md` | `fah-api` serves `/web`; multi-stage image; route ordering; boot check | Opus | WAITING |
| 2 | `p5-02-auth-session.md` | Argon2id password, session cookie, login/logout, cookie on REST + WS (heavy) | Opus | WAITING |
| 3 | `p5-03-frontend-foundation.md` | Vite/TS/Preact shell, typed client, socket manager, size gate, login page | Opus | WAITING |
| 4 | `p5-04-dashboard-and-lists.md` | Dashboard and Lists — proves tiles, charts, tables, mutations | Opus | WAITING |
| 5 | `p5-05-filtering-pages.md` | Custom Rules, Policies, Clients, Rule Tester | Opus | WAITING |
| 6 | `p5-06-runtime-pages.md` | Cache, Performance, Upstreams | Opus | WAITING |
| 7 | `p5-07-settings-and-diagnostics.md` | Settings (curated + raw), Health, Memory, Live Feed | Opus | WAITING |
| 8 | `p5-08-phase5-verification.md` | Bundle and image budgets, RB5009 validation, e2e against a live API | Opus | WAITING |

## Quality gates for this phase

The workspace gates in [plan/CLAUDE.md](../../CLAUDE.md) still apply in full and
are still the ones that decide `DONE`:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

From `p5-03` on, a task that touches `dashboard/frontend/` must **also** pass,
from that directory:

```sh
npm run typecheck     # tsc --noEmit, no implicit any
npm run build         # must succeed, and must fail over the size budget
```

The bundle budget is **150 KB gzip for the whole application** and it is
enforced by the build, not audited by hand — a build that exceeds it exits
non-zero. Treat a budget failure exactly like a clippy failure: fix it, or stop
and report.

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
5. **Budgets are drawn as markers, never as walls.** Nothing enforces memory or
   latency at runtime — the container runs `memory-high=unlimited`. A chart that
   implies a hard limit invites someone to set one.
6. **Bounded everything, on the client too.** The event feed holds a fixed ring;
   no page accumulates state with uptime.
7. **The sketch is not source.** [docs/dashboard/sketch/](../../../docs/dashboard/sketch/)
   settles layout and interaction. Do not import from it, and do not treat its
   figures as measurements.

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

- **Argon2id memory on a 1 GB box shared with RouterOS.** Each verification
  allocates its memory parameter; unthrottled login attempts are a
  memory-pressure vector, not merely a guessing one. Parameters are measured on
  the device, never copied from a guide, and login is rate-limited from the
  first commit that accepts a password (p5-02).
- **Image budget.** ≤ 30 MB, measured at 13.0 MiB today. A multi-stage build
  keeps the toolchain out, but assets, source maps and fonts all land in the
  final layer if nobody is watching (p5-01, p5-08).
- **Bundle budget.** 150 KB gzip is generous for this design and trivial to
  blow with one convenience dependency. The gate is in the build for that
  reason (p5-03).
- **Route ordering.** A catch-all SPA fallback that shadows `/api/v1/*` turns
  every API 404 into an HTML page and every client error into a parse failure.
  Ordering is asserted by test, not by inspection (p5-01).
- **Cookie on the WebSocket upgrade.** `?token=` puts a credential in logs. The
  dashboard must never use it; the upgrade takes the cookie (p5-02).
- **The curated Settings form drifting from the config.** A hand-written form is
  a subset by construction; the raw All-settings panel is what keeps an
  unexposed key visible rather than silently absent (p5-07).
