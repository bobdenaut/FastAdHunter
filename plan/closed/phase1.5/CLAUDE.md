# Phase 1.5 — Observability Persistence (pre-Phase 2 base)

**Objective:** persist the observability data long-term on the SSD so a future
Pi-hole-style dashboard has 30/60/90 days (REST-configurable) of everything to
draw from — query volume, block rate, query types, top domains/clients, cache
performance, upstream health, latency percentiles, and **process RAM/RSS over
time**. Plus the recorded soak follow-up: a byte-aware cache cap.

**Why this phase exists (before Phase 2):** Phase 1 shipped a working resolver
and a *live* API, but the aggregate time-series is a bounded **24-hour ring**
([../../closed/phase1](../../closed/phase1) → `fah-stats`), and the Prometheus
counters are ephemeral (reset on restart, built for an external scraper). A
dashboard needs durable, queryable history. Building that foundation now — a
"better base" — means Phase 2's UI work reads a stable data contract instead of
inventing one. The ~91h RB5009 soak (docs/code-review/phase1/p1-11-soak.md) is what
motivated this: it proved the system is bounded, and it also proved the value
of a persisted RSS/perf series (the soak did it *externally* with a curl loop;
this phase makes the router self-host it).

**Why this order:** the rollup + perf-sample *writers* first (they define the
on-disk contract), then retention config, then the read API that serves them,
then the cache cap and the multi-socket ingest scaling (independent hardening —
the ingest task *measures before it changes anything*), then verification last.

## What to persist (resolved with the user)

- **Cache contents → NO** (ephemeral by design, root CLAUDE.md hard rule #4).
  The cache **stats** (`GET /api/v1/cache` body:
  `entries/capacity/fresh/stale/expired/evictions/load_percent/hits/misses`)
  **→ YES**, sampled over time.
- **Prometheus raw counters → NO** — sample the *quantities* internally.
- **Query aggregates + RAM/perf → YES**, rolled up to flat JSONL on `/data`
  (ADR-0002: no embedded DB), bounded and pruned by `retention_days`.

Per-query events already persist (`/data/query_log/segments/`); their retention
is unchanged. Long-term *per-client* time-series is out of scope for the base —
revisit with the UI.

**Correction (found during p1.5-07 verification):** the original wording here said
"recent per-client is derivable from the raw log". It is not — nothing reads those
segments. `GET /api/v1/queries` serves the in-RAM ring only
(`ring_entries`, ≈ `ring_entries ÷ QPS` of reach), so the persisted segments are
write-only until a segment reader exists. Any Phase 2 work that plans to derive
per-client history from the raw log has to build that reader first.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p1.5-01-history-rollups.md` | Hourly→daily aggregate rollup store in `fah-stats`; `HourRollup`/`DailyTopN` PODs; boot/load/prune | Sonnet | DONE |
| 2 | `p1.5-02-perf-sample-series.md` | `Metrics::snapshot()` + histogram percentiles; binary sampler (RSS + cache port + upstream); `PerfSample` persisted via `fah-stats` | Opus | DONE |
| 3 | `p1.5-03-history-retention-config.md` | `[history]` config + REST-live-settable retention (30/60/90); CONFIGURATION.md | Sonnet | DONE |
| 4 | `p1.5-04-history-query-api.md` | `GET /api/v1/history/{summary,perf,top}`; API.md | Sonnet | DONE |
| 5 | `p1.5-05-cache-byte-cap.md` | Byte-aware cache cap so the ceiling respects the 128 MB budget under adversarial input + sustained-throughput measurement | Opus | DONE |
| 6 | `p1.5-06-reuseport-multisocket-ingest.md` | MEASURED — ingest ruled out as limiter; reconfirmed under CPU saturation (hot-set hammer: `fastadhunter` 65.8% of box via `/tool profile`, 4 cores even ~85%, ~15-16k QPS, conntrack 1.6% of max); deferred, recipe retained (`docs/code-review/phase1/p1.5-06-review.md`) | Opus | DONE |
| 7 | `p1.5-07-verification.md` | Unit tests (rollup math, prune, sampler), e2e (populate→query history), on-device soak proving disk- and memory-bounded — soak PASSED (RSS plateau 104.5 MiB, 6.5h slope negative); found + fixed a `/metrics` cache-outcome bug (`docs/code-review/phase1/p1.5-07-review.md`), then verified the whole phase on-device on 0.2.4 (`docs/code-review/phase1/p1.5-08-0.2.4-deploy-verification.md`) | Sonnet | DONE |

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

**Definition of done:** after a day of traffic, `/data/history/` holds hourly
rollups and per-interval perf samples pruned to `retention_days`;
`GET /api/v1/history/summary|perf|top` return chart-ready series including
RSS and cache stats over time; changing `history.retention_days` via
`POST /api/v1/config` takes effect live; the sampler adds no unbounded memory
(hard rule #4) and the cache byte-ceiling respects the 128 MB budget under the
`test_aleator.py` worst case; workspace gates green.

**Key risks:** sampler must never touch the hot path (it reads snapshots on a
timer, like the existing telemetry poll) — mitigation: no per-query work, all
writes off-thread on the flush cadence; disk growth must stay bounded —
mitigation: reuse the `SegmentWriter` prune pattern, verify on-device;
histogram percentiles are estimates from fixed buckets — document the
resolution, don't imply exactness.

**Naming:** `phase1.5` sorts before `phase2` under the plan's
lowest-number-first rule; tasks use the `p1.5-NN` prefix.
