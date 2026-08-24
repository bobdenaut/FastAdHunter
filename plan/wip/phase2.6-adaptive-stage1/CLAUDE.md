# Phase 2.6 — Adaptive DNS Upstream Selection, Stage 1

**Objective:** Implement the accepted Stage 1 specification
([docs/design/adaptive-upstream-selection.md](../../../docs/design/adaptive-upstream-selection.md),
the authoritative gate for this phase): per-endpoint health state, penalty on
repeated transport failure, skipping penalized endpoints, on-path recovery
probing, `resolve_host` isolation, telemetry — behind `strategy = "adaptive"`,
opt-in. Then decide the gates: implementation/merge gates on the dev box,
deployment/default-flip gates on the RB5009. Stage 2 (RTT/EWMA) and Stage 3
(hedging) are candidate designs only and are **not** in this phase.

**Prerequisites:** Phase 2.5 closed; the S1-G4 run-length window
(`upstreams[].failure_runs`, p2.5-06) collecting on-device. Both hold.

**Why this order:** config surface and the pure health core first — they are
independent and the core is the riskiest logic (packed word, CAS loop, the
hard invariant), so it is tested in isolation before any I/O touches it.
Selection and outcome classification next, each pure. Pool integration wires
them under the new strategy without touching `fallback`. Telemetry and docs
make it observable. Then the three measurement tasks that decide the merge
gate (microbench, injected-failure bench, null A/B). Deployment and soak
decide G2 tiers 2–3, G4 and G5; the default flip is last and separately
approved.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p2.6-01-config-surface.md` | `strategy = "adaptive"` and `penalty_failures` parse, default, validate and override from env; 8-endpoint cap; `timeout_ms` range; the nine `DnsUpstreamsConfig` literals gain the field | Opus | DONE |
| 2 | `p2.6-02-health-core.md` | `Health` 64 B packed word, policy struct, pure transition function as a saturating CAS loop; S1.3 table pinned by tests | Opus | DONE |
| 3 | `p2.6-03-selection-probe.md` | One-pass config-order selection with lazy clock, `claim: bool`, hard invariant, single `Probing` claim per endpoint, forced-use recording | Opus | DONE |
| 4 | `p2.6-04-outcome-classification.md` | Transport outcomes map to `Outcome`; RCODE is success; `Record`/`Ignore` health mode; connection lifecycle never feeds health | Opus | DONE |
| 5 | `p2.6-05-pool-integration.md` | `forward` runs Stage 1 under `adaptive` (one probe per query, `Ignore` never claims); `resolve_host` isolated; `fallback` untouched, its test assertions unmodified | Opus | DONE |
| 6 | `p2.6-06-telemetry.md` | `state`, `penalty_round`, `penalties`, `penalized_seconds_total`, `probes`, `probe_successes`, `family` on `/telemetry`; `/health` degraded = no endpoint Healthy | Opus | DONE |
| 7 | `p2.6-07-docs.md` | CONFIGURATION.md, API.md, measurement-traps.md, CONTEXT.md updated — each edit owner-approved at execution time | Opus | WAITING |
| 8 | `p2.6-08-microbench.md` | S1-M: healthy-path selection cost `adaptive` vs `fallback`, pinned, zero allocations — G2 tier 1 | Opus | WAITING |
| 9 | `p2.6-09-injected-failure-bench.md` | S1-G3 scenarios pass; net timeout cost avoided reported in two rows | Opus | WAITING |
| 10 | `p2.6-10-null-ab.md` | Harness noise band N from suite S1-N; G2 tier 3 threshold frozen or dropped | Opus | WAITING |
| 11 | `p2.6-11-optin-deploy-soak.md` | `adaptive` deployed opt-in; G2 tiers 2–3, G4 and G5 decided from on-device evidence | Opus | WAITING |
| 12 | `p2.6-12-default-flip.md` | `adaptive` is the default; `fallback` path deleted; docs and project state updated | Opus | WAITING |

## TASK START / PHASE CONTEXT

Before starting a task:

1. Read the current task file completely **and its implementation plan**,
   `p2.6-NN-<slug>-plan.md`, beside it. The plan file carries the exact
   symbols and line ranges, the resolved contradictions (C1–C7) and the final
   timing terminology; where the two differ, the plan file wins.
2. Read the current phase status/table.
3. Read the spec sections the task file names. Do not read the whole spec.
   Timing terms are fixed by spec S1.6 and used identically everywhere:
   `attempt_timeout` = the existing per-leg value (`timeout_ms`);
   `attempt_bound_ms = ATTEMPT_LEGS × timeout_ms` bounds one attempt;
   `PENALTY_BASE = 10 × attempt_bound_ms`.
4. Read the **Implementation Summary** from the code-review files of previously completed tasks in the same phase that are relevant to the current task.
5. If the current task declares an explicit dependency (`Depends on: pY-XX`), always read that dependency's Implementation Summary.
6. Read full code-review findings only when the current task depends on a finding, deferred item, constraint, or decision that is not fully captured by the Implementation Summary.
7. Read any explicitly referenced architecture, security, API, configuration, or known-debt documents.

Do not read unrelated completed tasks or full review files merely because they belong to the same phase.

Do not re-litigate decisions already settled by the spec or by previous tasks unless new evidence directly conflicts with them. The spec's Stage 1 scope (S1.1) is binding: nothing from Stage 2 or Stage 3 is introduced, not even as an inert key.

## TASK COMPLETION / REVIEW HANDOFF

When a task implementation is complete:

1. Do not summarize or describe the implementation in the chat.
2. Do not list changed files, implementation details, design decisions, benchmarks, tests, or findings in the chat.
3. Create the required code-review file immediately:
   `docs/code-review/phase2.6/<task-name>-review.md`
4. At the beginning of that review file, include a concise **Implementation Summary** describing:
   - what was implemented;
   - the relevant files/modules changed;
   - important design decisions;
   - tests/benchmarks run, if any;
   - any known limitations or deferred items.
5. The Implementation Summary may be based on the implementation and test results, but do not perform or document code-review findings yet.
6. Then stop. Do not perform the code review yet.
7. The only chat response after completing the task should be:

   `Task done. Report written to docs/code-review/phase2.6/<task-name>-review.md. Awaiting "start code review".`

8. Do not start the code review, add findings, or modify the findings section until the user explicitly says:
   `start code review`

When `start code review` is received, perform the CODE REVIEW procedure defined below and update the same review file.

**Do not implement, modify, revert, refactor, or otherwise change any code, configuration, tests, documentation, or architecture findings identified during the review without the user's explicit approval.**

The review phase is analysis and reporting only. After the review, stop and wait for explicit instructions before applying any fixes.

## CODE REVIEW

Every task must have a corresponding `*-review.md` file.
The file must be saved under `docs/code-review/phase2.6/`.

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

   `Fixes applied. Review updated: docs/code-review/phase2.6/<task-name>-review.md. Gates green.`

6. Then stop and wait for further instructions.

Do not proactively report individual fixes, changed files, test counts, implementation details, or review findings in chat after an approved-fix cycle. That information belongs in the review file.

**Definition of done:** under `strategy = "adaptive"` a dead endpoint costs at
most `penalty_failures` timeouts and is then skipped at one relaxed load; every
endpoint Penalized still sends a query; a probe is claimed exactly once per
endpoint per deadline, at most once per query, and never by `resolve_host`; a
recovered endpoint ahead of a Healthy one is probed by the next query that
reaches it; `resolve_host` moves no health state; `fallback` is bit-identical
to today and its test assertions are unmodified; all S1-G1 tests (#1–#17)
pass; S1-G2 tier 1 and S1-G3 pass on the bench; S1-N reports N; `adaptive`
has run opt-in on the RB5009 through the 7-day soak with G2 tiers 2–3, G4 and
G5 decided and recorded; the default flip is a separate approved commit;
gates green throughout.

**Key risks:** the packed word — a `fetch_add` on it corrupts `timestamp_ms`
(mitigation: CAS loop only, saturation test #11); the hard invariant — a
forced failure that extends the deadline turns a WAN blip into a resolver
that never probes (mitigation: test #6 asserts deadline and round unchanged);
`resolve_host` at 20 % of attempts — one missed `Ignore` and the health
signal is swamped (mitigation: test #7), and an `Ignore` call that claims a
probe leaves the word in `Probing` forever (mitigation: `claim = false`, test
#16); a two-pass "Healthy first, then due" selector never probes a recovered
primary while the secondary answers (mitigation: one pass, test #8 with a
Healthy endpoint behind the due one); the S1-G4 window may close with too
few runs to read — the spec says extend the window, not guess; every `.md`
edit in `p2.6-07` and `p2.6-12` needs the owner's explicit approval at
execution time (working agreement) — the tasks list them, they do not
pre-authorize them; every router command in `p2.6-11` is proposed, never run.

## Where this phase sits in the Adaptive DNS design

The spec describes three stages. Only the first is accepted; this phase
implements only the first.

```text
Adaptive DNS
│
├── Stage 1 — failure-aware selection        ← accepted; THIS PHASE
│   detect transport failure, penalize, skip, probe for recovery
│
├── Stage 2 — latency-aware selection        ← candidate only
│   RTT/SRTT per endpoint, banded ordering, family preference
│   gate: a durable DNS-level latency separation must be shown first;
│   if none, the outcome is a static family-preference rule, no RTT code
│
└── Stage 3 — hedging / slow-but-answering   ← candidate only
    delayed second attempt, first answer wins
    gates: hickory cancellation proven by test; a slow-but-answering
    population exists in real traffic; p99 win by a pre-agreed margin;
    amplification accepted cache-hit-adjusted
```

These are not three phases to be implemented in sequence. Stage 2 and
Stage 3 are candidate designs that must earn a detailed specification
through their benchmark gates, and may never be built — Stage 1 may already
have taken the win Stage 3 would chase. Stage 1 itself must earn its default
flip (S1-G4, S1-G5): if the run-length window shows only isolated losses,
not shipping is the correct outcome. The sequence is: implement Stage 1 →
measure and close it → decide Stage 2 and Stage 3 from that data. Nothing
from Stage 2 or 3 enters this phase, not even an inert config key
(`deny_unknown_fields` makes every key permanent).
