# Phase 2.5 — Pre-Adaptive Hardening

**Objective:** Close the operational risks and Adaptive DNS Stage 1 ship-gates
identified by the Global Architecture Review
([docs/code-review/Global Architecture Review-Reconciled.md](../../../docs/code-review/Global%20Architecture%20Review-Reconciled.md),
the authoritative gate for this phase). Two live-resolver defects (silent
listener death, list-refresh poisoning), the encrypted-transport fixes the
accepted Stage 1 spec depends on, the outcome telemetry Stage 1 needs to be
judged, the S1-G4 run-length instrumentation, and cheap hygiene. Adaptive
Stage 1 implementation itself and the Phase 3 decision/ADR package are
explicitly **not** in this phase.

**Why this order:** live-resolver protection first (a dead listener or a
poisoned ruleset is a today-problem, not a Stage 1 problem). Then the
encrypted-transport fixes (small, and everything Stage 1 assumes about
transports must be true before Stage 1 is specified against them). Then
observability (the SERVFAIL-served counter and run-length instrumentation
want **deployment time** — every day they run before Stage 1 lands is
measurement data for gate S1-G4, so a mid-phase deploy after `p2.5-06` is
recommended). The SWR lease check, hygiene, and verification close the phase.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p2.5-01-listener-resilience.md` | DNS listener loops survive transient socket errors; healthcheck exercises port 53 | Opus | DONE |
| 2 | `p2.5-02-list-refresh-integrity.md` | A fetched body is validated before it can replace the last-good `/data` copy; `parse_errors` reaches the API | Opus | DONE |
| 3 | `p2.5-03-encrypted-reconnect.md` | A timeout invalidates the pooled DoT/DoH connection; next exchange reconnects (S1.15 becomes true) | Opus | DONE |
| 4 | `p2.5-04-transport-error-kinds.md` | `io::ErrorKind` fidelity through encrypted transports; RCODE-is-not-a-failure pinned by test | Opus | DONE |
| 5 | `p2.5-05-outcome-telemetry.md` | Client-visible failure (SERVFAIL served) counted; per-query endpoint attribution on events | Opus | DONE |
| 6 | `p2.5-06-failure-runlength.md` | Per-endpoint failure run-length distribution observable via `/telemetry` (gate S1-G4 data source) | Opus | WAITING |
| 7 | `p2.5-07-swr-lease-check.md` | SWR refresh claim lease provably exceeds the worst-case upstream walk | Opus | WAITING |
| 8 | `p2.5-08-hygiene.md` | Tracked bearer token gone; layering guard covers the whole workspace; stale docs reconciled | Opus | WAITING |
| 9 | `p2.5-09-phase-verification.md` | Gates green, deployed, listener-death drill passed, S1-G4 collection running | Opus | WAITING |

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
   `docs/code-review/phase2.5/<task-name>-review.md`
4. At the beginning of that review file, include a concise **Implementation Summary** describing:
   - what was implemented;
   - the relevant files/modules changed;
   - important design decisions;
   - tests/benchmarks run, if any;
   - any known limitations or deferred items.
5. The Implementation Summary may be based on the implementation and test results, but do not perform or document code-review findings yet.
6. Then stop. Do not perform the code review yet.
7. The only chat response after completing the task should be:

   `Task done. Report written to docs/code-review/phase2.5/<task-name>-review.md. Awaiting "start code review".`

8. Do not start the code review, add findings, or modify the findings section until the user explicitly says:
   `start code review`

When `start code review` is received, perform the CODE REVIEW procedure defined below and update the same review file.

**Do not implement, modify, revert, refactor, or otherwise change any code, configuration, tests, documentation, or architecture findings identified during the review without the user's explicit approval.**

The review phase is analysis and reporting only. After the review, stop and wait for explicit instructions before applying any fixes.

## CODE REVIEW

Every task must have a corresponding `*-review.md` file.
The file must be saved under `docs/code-review/phase2.5/`.

Before marking a task `DONE`, review the implementation as a senior Rust reviewer with standards comparable to Servo/Tokio review.

Focus on:

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

   `Fixes applied. Review updated: docs/code-review/phase2.5/<task-name>-review.md. Gates green.`

6. Then stop and wait for further instructions.

Do not proactively report individual fixes, changed files, test counts, implementation details, or review findings in chat after an approved-fix cycle. That information belongs in the review file.

**Definition of done:** a DNS listener hit by a transient socket error keeps
serving; a killed listener is caught by the healthcheck instead of netwatch;
a 200-OK garbage list body cannot replace the last-good ruleset and the
failure is visible via the API; a blackholed DoT/DoH connection heals in one
timeout window; SERVFAIL-served and per-endpoint attribution appear in
`/telemetry` and on events; the failure run-length distribution is being
collected on-device; the tracked bearer token is gone and rotated; gates
green throughout.

**Key risks:** the listener-resilience change touches the hottest ingest
loops — the retry path must not add cost to the happy path (mitigation:
error-path-only changes, benches unchanged); telemetry additions must not
break the `/telemetry` invariants the 0.2.12 soak pinned (`hits + misses ==
pass + allow`) — restate them consciously if the new counters interact;
doc edits inside `p2.5-08` each need the owner's explicit approval at
execution time (working agreement) — the task lists them, it does not
pre-authorize them.
