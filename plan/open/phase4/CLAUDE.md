# Phase 4 — HTML Filtering

**Objective:** ROADMAP.md Phase 4: streaming HTML rewriting powered by
lol_html — element/cosmetic rules (`##`, `#@#`) activate in the Rule Engine
and are applied to HTML responses flowing through the Phase 2/3 HTTP pipeline.
The differentiator AdGuard Home lacks: filtering **inside** pages. Applied
only where required — non-HTML and selector-less traffic passes through
untouched, preserving the pass-through fast path.

**Why this order:** docs + config gating first (new subsystem — docs update in
the same change, per root CLAUDE.md). Cosmetic rule compilation before the
rewriter that consumes it (fah-rules owns all rule processing). Rewriter core
proven in isolation before it touches the proxy (streaming + bounded memory
must be solid first). Pipeline integration wires verdicts, policies, events
and stats together, then proof against budgets.

> **Built, dormant by default — owner decision 2026-09-19.** All five tasks
> are implemented and ship with `[html] enabled = false`. The code lands, the
> gate stays closed, and nothing on the deployed box changes until the owner
> switches it on at runtime (`POST /api/v1/config {"html": {"enabled": true}}`,
> no restart). This supersedes the 2026-09-15 park recorded in
> [ADR-0009](../../../docs/decisions/0009-phase-4-parked.md); its evidence
> still describes the deployment — interception is off and the deployed lists
> carry 720 URL rules and no cosmetic ones — which is why the default is off.
>
> Switching it on later needs **both** interception enabled for real clients
> **and** cosmetic/URL-path lists loaded; either alone leaves the rewriter with
> nothing to act on. Building it needs neither: every task is verified on the
> dev box, and on-device figures come from a probe-container bench run, not a
> household soak (see the p4-05 plan).

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Plan | Outcome | MODEL | STATUS |
|---|-----------|------|---------|-------|--------|
| 1 | `p4-01-html-scaffold.md` | [p4-01-html-scaffold-plan.md](p4-01-html-scaffold-plan.md) | `[html]` config + gating + doc/diagram updates; rewrite hook stub | Opus | WAITING |
| 2 | `p4-02-cosmetic-rules-activation.md` | [p4-02-cosmetic-rules-activation-plan.md](p4-02-cosmetic-rules-activation-plan.md) | Cosmetic rules compile into per-hostname selector sets (heavy) | Opus | WAITING |
| 3 | `p4-03-streaming-rewriter.md` | [p4-03-streaming-rewriter-plan.md](p4-03-streaming-rewriter-plan.md) | lol_html streaming rewriter: bounded, charset/encoding-aware (heavy) | Opus | WAITING |
| 4 | `p4-04-pipeline-integration.md` | [p4-04-pipeline-integration-plan.md](p4-04-pipeline-integration-plan.md) | Selective application in HTTP/HTTPS pipeline; policies, events, stats | Opus | WAITING |
| 5 | `p4-05-phase4-verification.md` | [p4-05-phase4-verification-plan.md](p4-05-phase4-verification-plan.md) | Rewrite budgets in PERFORMANCE.md, benches, e2e, RB5009 validation | Opus | WAITING |

Each plan is the step list for its task: anchors into the code as of 0.4.1,
decisions settled up front, tests, benches, the doc edits to list after the
gates, and the review-file hand-off. Read the task file first, then its plan.
The plans were written 2026-09-19 against workspace 0.4.1 under the
built-dormant decision above; re-check every line anchor before editing.

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
   `docs/code-review/phaseN/<task-name>-review.md`
4. At the beginning of that review file, include a concise **Implementation Summary** describing:
   - what was implemented;
   - the relevant files/modules changed;
   - important design decisions;
   - tests/benchmarks run, if any;
   - any known limitations or deferred items.
5. The Implementation Summary may be based on the implementation and test results, but do not perform or document code-review findings yet.
6. Then stop. Do not perform the code review yet.
7. The only chat response after completing the task should be:

   `Task done. Report written to docs/code-review/phaseN/<task-name>-review.md. Awaiting "start code review".`

8. Do not start the code review, add findings, or modify the findings section until the user explicitly says:
   `start code review`

When `start code review` is received, perform the CODE REVIEW procedure defined below and update the same review file.

**Do not implement, modify, revert, refactor, or otherwise change any code, configuration, tests, documentation, or architecture findings identified during the review without the user's explicit approval.**

The review phase is analysis and reporting only. After the review, stop and wait for explicit instructions before applying any fixes.

## CODE REVIEW

Every task must have a corresponding `*-review.md` file.
The file must be saved under `docs/code-review/phaseN/`.

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

   `Fixes applied. Review updated: docs/code-review/phaseN/<task-name>-review.md. Gates green.`

6. Then stop and wait for further instructions.

Do not proactively report individual fixes, changed files, test counts, implementation details, or review findings in chat after an approved-fix cycle. That information belongs in the review file.

**Definition of done:** a page loaded through the proxy (plain HTTP, or HTTPS
on an intercepted client) has its ad elements hidden/removed by cosmetic rules;
`#@#` exceptions honored; non-HTML responses and hosts with no applicable
selectors take the pass-through fast path with no added buffering; memory stays
bounded regardless of page size; rewrite overhead holds the budgets added to
PERFORMANCE.md; counts of now-active cosmetic rules visible per list in the API.

**Key risks:** HTML filtering only reaches traffic the proxy can see — plain
HTTP plus intercepted-client HTTPS (set expectations in docs; DNS/SNI layers
keep covering the rest); Content-Encoding — lol_html needs decoded bytes, so
candidate requests must negotiate identity encoding or stream-decompress
(decided and benched in p4-03/p4-04); page CSP can block injected styles
(mitigation: element removal path + documented limitation); lol_html selector
compilation cost per response (mitigation: compiled-selector caching per
hostname, bounded).
