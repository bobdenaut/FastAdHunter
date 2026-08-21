# Phase 0 — Workspace Skeleton

**Objective:** a compiling Cargo workspace with all 10 crates stubbed, the pure
domain types in place, config loading working, a binary that boots and answers
`--healthcheck`, and a distroless Docker image that runs it. Nothing resolves
DNS yet; nothing filters yet.

**Why this order:** foundations bottom-up along the dependency layers (L1 → L4):
types before the crates that use them, config before the binary that loads it,
binary before the image that ships it. Every task leaves the workspace green.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p0-01-workspace-skeleton.md` | Workspace + 10 stub crates, layering enforced, gates green | Opus | DONE |
| 2 | `p0-02-model-types.md` | `fah-model`: Query, Verdict, Client, QueryEvent compile + tests | Opus | DONE |
| 3 | `p0-03-common-logging.md` | `fah-common` error types + `fah-logging` tracing init | Opus | DONE |
| 4 | `p0-04-config-loading.md` | `fah-config`: TOML + defaults + env precedence, typed, tested | Opus | DONE |
| 5 | `p0-05-binary-bootstrap.md` | `fastadhunter` boots Tokio, loads config, `--healthcheck` works | Opus | DONE |
| 6 | `p0-06-docker-image.md` | Static musl build in distroless image, arm64 + amd64 | Opus | DONE |

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

**Definition of done:** `cargo test --workspace` green; `cargo run -- --healthcheck`
exits 0; `docker build` produces an image that starts, logs its config source,
and passes its healthcheck. No DNS, no API, no rules code exists yet.

**Key risks:** cross-compiling `aarch64-unknown-linux-musl` toolchain setup on
the Windows dev machine (mitigation: p0-06 accepts amd64-local proof + arm64
cross-compile check only; on-device RB5009 validation belongs to Phase 1).
