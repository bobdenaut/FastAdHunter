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
| 1 | `p3-01-cert-core.md` | CA generation, leaf minting + cache, PEM import, storage (heavy) | Fable | DONE |
| 2 | `p3-02-certificates-api.md` | `/api/v1/certificates` per reserved namespace; API.md updated | Fable | DONE |
| 3 | `p3-03-sni-filtering.md` | Blocked domains die at SNI — no decryption, works for every client | Fable | DONE |
| 4 | `p3-04-tls-interception.md` | Opt-in per-client MITM feeding the Phase 2 HTTP pipeline (heavy) | Fable | DONE |
| 5 | `p3-05-dot-doh-listeners.md` | DoT :853 + DoH listeners; Android Private DNS works | Fable | DONE |
| 6 | `p3-06-phase3-verification.md` | TLS budgets, e2e, RB5009 dst-nat 443 + CA install walkthrough. Probe campaign 2 (post-merge `e0c6071`): `p3-06-testing-plan.md`, gated by `p3-06-smoke-plan.md` layers 0–3 on the dev box first. Campaign 1 superseded — `docs/code-review/phase3/p3-06-testing-results.md`. **PARKED 2026-09-13 — the owner decided not to use the interception code.** This file is written around a full-mode deployment: CA install on a client device, intercepted HTTPS URL blocking, full-mode RAM, the pinned-app spot-check, the 24 h full-mode soak. **Its non-interception arms were not parked with it** — the dst-nat 443 steering, the DoT/DoH and SNI budget rows, the security suite, Private DNS and the deploy guide moved to row 11. Reviving this takes a new decision | Fable | PARKED |
| 6b | `p3-06-after-interception-impl.md` | p3-06 follow-up after p3-07…p3-09: probe scripts and smoke/testing plans off the dead `https.interception` key, runbook rows for the migration on the probe and the ADR-0008 device path (525 → exclude → splice), soak watch additions. No campaign re-run. **p3-10 now exists** (row 10) and owns the post-merge gaps and the performance characterization; §6 "Not owed" in that task file still reads "A new p3-10 task or test plan" and needs the same amendment. §4 Order items 1–4 and 6 all ran 2026-09-11 (scripts committed `223bf79`, B5–B10 applied, N1/N2/N4 PASS, N3 FAIL filed as a design finding, D1/D2 done) and stand. **PARKED 2026-09-13 — the owner decided not to use the interception code.** All that was left is item 5, N5–N6 inside a full-mode soak, and full mode is interception-on; it has no meaning under that decision. Nothing is expected to flip this, and reviving it takes a new decision | Fable | PARKED |
| 7 | `p3-07-interception-document.md` | `interception.json` + `GET`/`PUT /api/v1/interception`, atomic swap, one-boot migration (release N), `BASELINE_EXCLUSIONS` deleted — ADR-0008 §Phasing step 1 | Fable | DONE |
| 8 | `p3-08-client-cert-rejection.md` | Accept-side alert classification: `https` event `status 525` (`ClientCertRejected`); `UnknownCA` and every unclassified failure stay `0` — ADR-0008 step 2 | Fable | DONE |
| 9 | `p3-09-rejection-view-and-document-editor.md` | Dashboard: rejection view grouped by client and host with an exclude-exact-host action; Interception Document editor — ADR-0008 step 3 | Fable | DONE |
| 10 | `p3-10-post-merge-performance.md` | Post-merge coverage gaps (Track A — **all ten closed 2026-09-13**: six by a recorded read or owner decision, and A1/A3/A4/A7 by tests written here, each shown to fail with its own wiring broken and then reverted) and Phase 3 performance characterization (Track B). **B1 ran 2026-09-13** — `docs/code-review/phase3/p3-10-track-b1-x86.md` — and **does not close the W1 gate**: the splice rows moved 44–101% between two runs of identical code, so no percentage claim rests on them. Open here: the two DoT/DoH rows (the load generator does not exist) and splice RSS (no verified runner). B2 on the RB5009 stays BLOCKED on the deploy decision and on p3-11's dst-nat 443 rule; the N sweep gates on the shipped workload, W1. No production code changes; every new harness is its own deliverable | Fable | WAITING |
| 10b | `p3-10b-dot-connection-gauge.md` | Production-code follow-up from p3-10 A5. A DoT connection gauge separate from the TCP one, `active` / `peak` / `closed_oversize`, passed where `dot.rs:152` passes `None` today. Without it the first Phase 3 soak sets the final `dns.tcp_max_connections` default from half the traffic, and p3-10's B2 row "peak concurrent DoT connections" cannot be run at all. Shipped 2026-09-14 as `counters.dns_dot_connections` on `/api/v1/telemetry`, counted at accept so a stalled handshake is in the figure that sizes the 64-connection cap; review and fixes in `docs/code-review/phase3/p3-10b-dot-connection-gauge-review.md`. **Row 11's soak no longer waits on this one — only on 10c** | Fable | DONE |
| 10c | `p3-10c-acceptor-death-observation.md` | Production-code follow-up from p3-10 A9. The HTTP, HTTPS and API acceptors report an unplanned end into supervision — counted through `record_task_death`, never ending the run loop. **The mechanism is not settled**: `Supervised` owns its `JoinHandle` and all three handles are private and needed by their own `shutdown()`, so the task's plan picks a route and justifies it before any code. **Must be DONE before row 11's seven-day soak starts** — an acceptor dying silently on day three gives a soak that looks clean and measured nothing | Fable | WAITING |
| 11 | `p3-11-verification-sni-scope.md` | Phase 3 verification for what actually runs: DNS, plain HTTP, HTTPS filtered at the SNI with no decryption, DoT/DoH. Carries p3-06's non-interception arms — dst-nat 443 and its rollback, SNI/DoT/DoH budget rows, the security suite including "interception disabled ⇒ byte-identical splice", Private DNS, the deploy guide's HTTPS section — plus a **seven-day soak** on the deployed build. The dev-box half runs today; the device half waits on the deploy decision, and **the soak additionally waits on row 10c** — 10b landed 2026-09-14, so the DoT gauge is now an instrument the soak reads rather than one it waits for, and the week is still that gauge's first real use under household traffic, not a re-verification of it | Fable | WAITING |

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
setup; a phone with Private DNS set to the container resolves over DoT; budgets
hold; SECURITY.md promises verified (CA key never leaves `/config`, public-only
export) — and, because it is now a promise rather than a default, that
interception is off: an empty client scope splices byte-for-byte.

**Interception is not part of done.** The owner decided on 2026-09-13 not to use
it. The two clauses that depended on it — a managed client with the CA installed
getting URL-level filtering inside HTTPS, and pinned apps surviving it — were
removed rather than left as criteria nobody intends to meet. The code ships
compiled and a later decision restores both, together with p3-06 and p3-06b.

**Key risks:** certificate-pinned apps break under interception (mitigation:
interception is opt-in per client + `exclude_domains` in the Interception
Document, edited live via `PUT /api/v1/interception` or the Settings card, and
a client refusing our leaf surfaces as `status 525` in the Live Feed rejection
view — ADR-0008; SNI path is the default and breaks nothing); Android CA install
friction (mitigation: p3-06 walkthrough with screenshots, and the
imported-real-cert route needs no CA install; Private DNS **hostname mode
does validate** — the CA route serves an SNI-minted leaf, p3-05 decision 3;
only Android's "automatic" mode validates nothing);
encrypted ClientHello (ECH) hides SNI on some traffic, and with no recoverable
original destination inside the container (measured on-device: `SO_ORIGINAL_DST`
returns `ENOENT`, `docs/routeros-traps.md`) such a connection is closed, not
forwarded (mitigation: `[https.sni] no_sni` classifies it; the DNS layer still
catches those domains).
