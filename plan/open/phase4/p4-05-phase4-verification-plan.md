# P4-05 — Phase 4 Verification — Implementation Plan

Task file: [p4-05-phase4-verification.md](p4-05-phase4-verification.md).
Depends on p4-04 ([plan](p4-04-pipeline-integration-plan.md)); read all
four earlier review files' Implementation Summaries and Measurements
sections — this task turns their numbers into budgets. Written 2026-09-19
against workspace 0.4.1; re-check anchors before editing.

## Decision — built, dormant by default

Owner decision 2026-09-19 (phase [CLAUDE.md](CLAUDE.md)): implement the whole
phase; the code ships with `[html] enabled = false` and stays dormant on the
deployed box. This task therefore proves two things on the device: that the
dormant build costs nothing (a soak with the gate closed), and what a rewrite
costs when it runs (a probe-container bench run). It never switches the
feature on for the household, never asks for interception, never loads
cosmetic lists on the router — Step 4 below.

This supersedes two lines of the task file, which stays as written: its
scope item "RB5009 validation: household soak with HTML filtering on" and
its acceptance criterion "on-device numbers recorded" are met by 4A (dormant
soak) and 4B (probe-container bench), not by a household run with the
feature on. The phase [CLAUDE.md](CLAUDE.md) records the decision; the
review file restates it so a later reader does not look for a soak that was
never meant to happen.

## Standing rules that bite here — read twice

- **The RB5009 is off limits.** No `/container …`, no firewall, no `/system`,
  no config edit, not with permission, not "just this once". Read-only
  queries (`/container/print`, `/system/resource/print`, `/log print`,
  `/file print`, API `GET`s) are fine. When a change is needed: propose the
  exact commands, say what each does and when it takes effect, and stop.
- **`scp` needs a fresh yes** per use.
- **Budgets are commitments.** A row lands with the number that proves it,
  never with a hoped-for figure. `TBD — must be measured during
  verification` is the table's own convention for a device figure that is
  not in yet; use it, never a guess.
- Every figure carries corpus, workload and device, and says how it can be
  superseded. Dev-box numbers are converted with the ~9× factor and labelled
  converted; RouterOS clock readings never calibrate anything.
- `.md` edits after the gates, one yes per file. No commit, no tag.

## Read before editing

| Need | Where |
| ---- | ----- |
| Budget table shape and conventions (MB vs MiB, `TBD` wording, the `>10 %` rule) | PERFORMANCE.md §Budgets (~33–86), §Converting dev-box numbers (~122), §Measuring on the RB5009 (~328) |
| Bench and soak traps | docs/measurement-traps.md (whole file — it is the reference for this task) |
| Router facts, probe procedure, what the owner runs | docs/routeros-traps.md; docs/deploy-rb5009.md (build + deploy sections only) |
| Precedent soak reports | `docs/code-review/phase2/soak-0.2.16-72h/report.md` (the 72 h / 6 h pull cadence), `docs/code-review/phase3/soak-0.3.3/`, `docs/code-review/phase2.6/soak-0.3.4/`, `docs/code-review/phase3/p3-06-testing-results.md` (the `certs_mint` probe run) |
| E2E rigs | `crates/fastadhunter/tests/{http_e2e,e2e_https,html_e2e}.rs`, `tests/common/mod.rs` — find how lists are served to the binary under test |
| Docs that still call HTML filtering future or parked | `rg -n -i "parked\|phase 4\|html filtering\|cosmetic" README.md ROADMAP.md CONTEXT.md docs/*.md` |

## Deliverables

1. PERFORMANCE.md budget rows, each backed by a bench and a device figure.
2. Benches named after the rows, A/B-able against `main`.
3. E2E over a real EasyList cosmetic sample, both pipelines, refresh under
   traffic.
4. RB5009 household soak with HTML filtering on — owner-executed deploy,
   read-only observation, written report.
5. Docs: what HTML filtering reaches and what it cannot; README, ROADMAP,
   project-state, ADR status.

## Step 1 — budget rows (values come from evidence, not from this plan)

Proposed rows for PERFORMANCE.md §Budgets, in the table's existing style.
Fill the Budget column from the p4-03/p4-04 bench tables **after** the
device run; until then the RB5009 column reads `TBD — must be measured
during verification`.

| Metric | Budget (set from evidence) | Bench that exercises it |
| ------ | -------------------------- | ----------------------- |
| **HTML** rewrite added latency per document, p99 (100 KiB, 300 selectors) | ≤ measured dev p99 × 9, rounded up to the next ms | `html_rewrite_latency` |
| **HTML** rewrite throughput floor (1 MiB, 300 selectors) | ≥ a floor above the household WAN rate, from the device figure | `html_rewrite_throughput` |
| **HTML** pass-through overhead with filtering enabled, non-HTML body | within noise of `http_opaque_body` on `main` (state the noise band you measured) | `http_opaque_body` A/B with the gate on |
| **HTML** cache-miss compile cost (300 selectors) | ≤ measured × 9 | `compiled_set_build` |
| **HTML** selector cache memory ceiling | `max_selector_cache × bytes-per-entry`, computed from p4-03's measured entry size at 300 selectors | `compiled_set_build` reports heap bytes |
| **HTML** rewrite buffers, worst case | `max_concurrent_rewrites × 256 KiB` = 16 MiB at defaults | property of the constants; the p4-03 bounded-memory test is the check (the dormant soak never rewrites) |
| RAM steady-state (existing row) | unchanged budget; add the measured value for the dormant build (`html.enabled = false`) | dormant soak (4A) |

The "≤ measured × 9" pattern is deliberate: a budget written from the
number that proves it, with a stated conversion, is one a later reader can
falsify. Do not tighten a budget to make it look ambitious.

## Step 2 — benches

The per-crate layout is the convention (`crates/*/benches/`); the root
`benches/` directory is empty and stays that way — do not "fix"
PERFORMANCE.md's `benches/` wording unasked, list it as a doc nit.

- Extend `crates/fah-http/benches/html_rewrite.rs` so every row above maps
  to one named group; keep the p4-03 groups.
- A/B protocol per docs/measurement-traps.md: check out `main`, run, save
  the criterion output outside the tree; check out the branch, run; compare
  medians. Never criterion's stored baseline.
- The `>10 %` rule (CONTRIBUTING.md ~68) now covers the HTML groups: say so
  in PERFORMANCE.md's sentence under the table.

## Step 3 — e2e (`crates/fastadhunter/tests/html_phase4_e2e.rs`)

Fixture: a ~500-line EasyList cosmetic sample (`##`, domain-scoped `##`,
`#@#`, a few `#?#` to prove they are skipped), attribution and licence in a
sibling `.txt`. Serve it to the binary the way the existing e2e serves lists
(check `tests/common/mod.rs`; if lists are only fetched from URLs, add a
loopback list server helper there, not in the test).

| Case | Assert |
| ---- | ------ |
| Page through plain HTTP | matched elements removed, `<style>` present, event `rewritten` with the selector count |
| Same page through the intercepted leg (`--all-features`, `e2e_https.rs` rig) | identical rewritten output |
| `#@#` exception host | the excepted element survives, others still hidden |
| Non-HTML asset | byte-identical, `skipped: not_html` |
| List refresh mid-traffic: loop fetching the page while `POST /api/v1/lists/{id}/refresh` swaps in a fixture with one more selector | zero failed responses; after the swap the new selector applies; `selector_cache_misses` grew by the number of distinct fingerprints, not by the number of requests |
| Extended rule in the fixture | counted in `rules_inactive`, never applied |

## Step 4 — RB5009 validation (dormant deploy + probe-container bench)

The feature is **not** switched on for the household. No interception, no
cosmetic lists, no `enabled = true` on the router. Two things are proven on
the device instead.

### 4A — the dormant build costs nothing

1. Build the arm64 image exactly as docs/deploy-rb5009.md says; record the
   image size against the 30 MB budget (13.0 MiB at 0.4.x).
2. **Propose** the deploy commands (container stop / image replace / start,
   in RouterOS syntax, with what each does and when it takes effect). Stop.
   The owner runs them. No other soak may be running.
3. Confirm `html.enabled` is `false` with `GET /api/v1/config` — never by
   reading or editing the TOML inside the container.
4. Baseline pull at T0, then every 6 h for 72 h (the 0.2.16 precedent):
   `GET /api/v1/telemetry` (`http.html` counters), `GET /api/v1/debug/memory`,
   `/metrics`, `/container/print`, `/system/resource/print`. Save raw pulls
   under `docs/code-review/phase4/p4-05-soak-<version>/`.
5. Report `docs/code-review/phase4/p4-05-soak-<version>.md` in the
   docs/code-review/CLAUDE.md structure, tables only: RSS delta vs the 0.4.x
   soak baseline (MiB, with MB beside it), every `http.html` counter — all
   must read zero — HTTP pass-through counters and latency unchanged, and
   whatever docs/measurement-traps.md says to check (boost, page cache,
   allocator residual). This is the evidence for the pass-through overhead
   row and the RAM steady-state row.

### 4B — rewrite figures from the probe container

1. Build the `html_rewrite` criterion binary for arm64 (musl static, like the
   release) and the `compiled_set_build` group with it.
2. **Propose** the probe-container commands per docs/routeros-traps.md
   §probe-container procedure, the way `certs_mint` was measured on
   2026-09-04. Stop. The owner runs them and pastes the output, or you read
   it from `/file print` if it is written to a file.
3. Record: corpus = the bench generator seed and parameters; workload = idle
   box, one core; device = RB5009. Compare each figure with the dev-box
   number and the ~9× factor, and say whether the factor held.
4. Report `docs/code-review/phase4/p4-05-probe-bench.md`, tables only. These
   numbers fill the RB5009 column for the latency, throughput and
   compile-cost rows — measured, not converted.

Anything that needs the feature switched on — cache hit ratio under real
browsing, `skipped: encoded` as the decompressor decision input, the minted-
leaf interplay with intercepted clients — is **not** measured here. Write
those rows as `TBD — needs the feature switched on` so a later reader sees a
decision, not a gap.

## Step 5 — docs (list, then wait)

| File | Edit |
| ---- | ---- |
| PERFORMANCE.md §Budgets | The rows from Step 1; the `>10 %` sentence; nothing else in the file. |
| docs/deploy-rb5009.md, docs/0.4.0-install.md (or its successor) | One short section: HTML filtering ships **off** (`[html] enabled = false`) and is switched on at runtime with `POST /api/v1/config {"html": {"enabled": true}}` once interception and a cosmetic list are in place; it reaches plain HTTP and intercepted clients; it cannot reach spliced HTTPS; injected styles lose to a page's own `!important` rules and to a CSP that blocks inline styles (removal still applies); procedural cosmetics are not applied; candidate documents are fetched uncompressed. Confirm via `GET /api/v1/config`, never by hand-editing the TOML. |
| README.md | Row 4 of the phase table, the tech-stack row (~752), the phase list (~780), and the sentences at ~19, ~207, ~441 that say HTML filtering is parked or that a router cannot do cosmetic filtering. |
| ROADMAP.md §Phase 4 | Heading loses `PARKED`; the two "what it would have delivered" bullets become checked deliverables; the "Reviving it needs both" paragraph becomes "built 2026-09-19, dormant by default (`[html] enabled = false`); switching it on needs both interception and cosmetic lists". |
| docs/project-state.md | The Phase row: rewrite, never append. |
| CONTEXT.md | §Rule and §Pass-through wording from p4-01 re-checked against what shipped. |
| docs/decisions/0009-phase-4-parked.md | Status line at the top: `Superseded <date> — <decision reference>`. Body untouched. Whether a new ADR records the revival is the owner's call; propose it, do not write it unasked. |
| Cargo workspace version | A feature phase is a minor bump (0.4.x → 0.5.0). Owner's call; propose with the release. |

Acceptance grep after the docs land:

```sh
rg -n -i "parked|future work|would have|not being built" README.md ROADMAP.md CONTEXT.md ARCHITECTURE.md PERFORMANCE.md docs/project-state.md docs/deploy-rb5009.md
```

Every remaining hit must be inside ADR-0009's own body or a dated history
line.

## Step 6 — gates, review file, stop

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
cargo bench -p fah-http --bench html_rewrite --bench proxy --bench intercept   # A/B vs main
```

Review file `docs/code-review/phase4/p4-05-phase4-verification-review.md`:
the budget table as landed with the evidence pointer per row, the e2e
matrix, a link to the soak report, and the list of doc edits done and
pending. If the soak has not run when the code is done, mark the task
`AWAITING SOAK` in the phase table's cell **only when told to** — the cell
must name what flips it (the 72 h pull set).

Chat line:

`Task done. Report written to docs/code-review/phase4/p4-05-phase4-verification-review.md. Awaiting "start code review".`

## Out of scope

- Any new functionality. A defect found by the soak is a finding in the
  review file, fixed only after an explicit go.
- Streaming decompression: `skipped: encoded` only exists once the feature
  is switched on, so the decision stays open; when that number exists it is
  a backlog item with the figure attached, not a change here.
- Extended/procedural cosmetics.

## Phase close

When every row in the phase table is `DONE` (or `PARKED` with its decision
recorded), the selector moves `phase4` to `closed/` per plan/CLAUDE.md and
reports "`phase4` done, gates GREEN. Waiting for approval to commit." One
commit covers code, docs and the folder move; pushes go to both `origin` and
`backup`, and only after the yes.
