# Phase 3 — HTTPS + Certificates + Encrypted DNS Listeners

**Objective:** ROADMAP.md Phase 3: certificate machinery (generate CA, import
PEM/PFX, export CA, status — rustls + rcgen + x509-parser, no hand-rolled
crypto), SNI-level HTTPS filtering for everyone, full HTTPS interception for
managed clients (opt-in, per-client, never default), and DoT/DoH **listeners**
so Android Private DNS points at us. Operating mode `dns+http+https` becomes
real.

**Why this order:** certificate core first (everything else consumes it), its
API second (small, unblocks dashboard-side work), then SNI filtering (big
value, zero interception risk), then interception (the hard, sharp tool, on a
proven base), then encrypted DNS listeners (need the cert story for clients to
validate), verification last.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p3-01-cert-core.md` | CA generation, leaf minting + cache, PEM/PFX import, storage (heavy) | Opus | WAITING |
| 2 | `p3-02-certificates-api.md` | `/api/v1/certificates` per reserved namespace; API.md updated | Opus | WAITING |
| 3 | `p3-03-sni-filtering.md` | Blocked domains die at SNI — no decryption, works for every client | Opus | WAITING |
| 4 | `p3-04-tls-interception.md` | Opt-in per-client MITM feeding the Phase 2 HTTP pipeline (heavy) | Opus | WAITING |
| 5 | `p3-05-dot-doh-listeners.md` | DoT :853 + DoH listeners; Android Private DNS works | Opus | WAITING |
| 6 | `p3-06-phase3-verification.md` | TLS budgets, e2e, RB5009 dst-nat 443 + CA install walkthrough | Opus | WAITING |

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

**Definition of done:** any client gets SNI-level HTTPS blocking with zero
setup; a managed client with the CA installed gets full URL-level filtering
inside HTTPS; a phone with Private DNS set to the container resolves over DoT;
banking/pinned apps keep working (exclusions honored); budgets hold; SECURITY.md
promises verified (CA key never leaves `/config`, public-only export).

**Key risks:** certificate-pinned apps break under interception (mitigation:
interception is opt-in per client + exclusion list ships with known pinned
domains; SNI path is the default and breaks nothing); Android CA install
friction (mitigation: p3-06 walkthrough with screenshots; DoT needs no CA);
encrypted ClientHello (ECH) hides SNI on some traffic (documented limitation —
DNS layer still catches those domains).
