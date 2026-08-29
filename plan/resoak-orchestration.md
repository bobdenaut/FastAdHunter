# Orchestration — p2.6-11 early close, phase-5 finish, fix, re-soak

**Owner decision 2026-08-29:** the p2.6-11 L.3 soak is terminated early
(~day 5 of 7). The RSS-excursion cause is identified and reproduced
([phase2.6-audit.md](../docs/code-review/phase2.6/phase2.6-audit.md)
§"Excursion cause — FOUND"); two more observation days add no information.
This file sequences the follow-up. Stages run in order; each names the
agent expected to execute it. Every stage obeys the global rules below.

## Global rules (bind every stage)

- **RB5009 is off limits.** Read-only GETs against the FAH API need no
  asking. Any change (container, firewall, config): propose the exact
  commands, explain each, stop — the owner runs them. One router
  intervention total, in Stage 4.
- **No `.md` created or edited without an explicit owner yes**, except a
  task's own review file under `docs/code-review/`. This plan file was
  owner-commissioned.
- **No commit/push/tag without a fresh go for that changeset.** Approved
  pushes go to both remotes (`origin` + `backup`).
- **API keys never enter the repo.** The fah-next key is owner-held; redact
  before committing any tooling.
- Evidence goes to `docs/code-review/phase2.6/` (soak) or
  `docs/code-review/phase5/` (dashboard), with corpus, workload, device.
- The audit file `phase2.6-audit.md` lives on **main** — edit and commit it
  via a main worktree, never onto a phase branch.

## Stage 0 — Fable, now: close p2.6-11 as terminated-early

Precondition: none. Router: read-only only.

1. **Final read-only pull** from the dev box (owner-held key):
   `GET /health`, `GET /api/v1/telemetry`, `GET /api/v1/debug/memory`,
   `GET /api/v1/history/perf?from=2026-08-25T07:57:02Z&to=<now>&stride=1`
   (paginate past `max_points=5000` if needed — full series, boot to pull).
2. **Commit the raws** as
   `docs/code-review/phase2.6/p2.6-11-session/soak/pull5-final-*.json`
   beside pulls 1–4 (already committed, `91ef00d`). This closes F7
   completely.
3. **Record the termination** in `phase2.6-audit.md` (main worktree):
   - soak **terminated early at owner decision, day ~5–6 — NOT a PASS**;
   - **W3 = FAIL stands** exactly as the F2 ruling fixed it;
   - W4–W7 classification is superseded by the found cause: scheduled
     full-body list re-downloads (transient, in-arena, reproduced);
   - acceptance of `adaptive` transfers to the Stage-4 re-soak.
4. Propose the matching closing section for
   `p2.6-11-optin-deploy-soak-review.md` and **wait for the owner yes**
   before writing it.
5. fah-next keeps serving untouched until Stage 4.

Acceptance: final pull committed; termination recorded; no router writes.

## Stage 1 — Opus, dev box: p5-10 Stage A

Precondition: Stage 0 recorded. Router: **no contact at all** — every check
runs against a local `fah-api`.

Execute **Stage A exactly as scoped** in
[p5-10-phase5-verification.md](wip/phase5/p5-10-phase5-verification.md)
§"Execution split": end-to-end tests; route-ordering regression; bundle
measurement (gzip + brotli, chunks, uPlot separately, login path); image
build + size + no-Node/no-toolchain proof; page-by-page figure trace;
route-scoped fetching table; socket-load run; mobile pass over the LAN
against the dev-box server. Evidence to `docs/code-review/phase5/` as the
task file directs; Stage B rows stay open, task sits `AWAITING SOAK`.

**If every Stage-A check passes:** propose the `phase5-10 → main` merge and
**stop for the owner's go** (merge + push to both remotes are
owner-approved actions). If anything fails: fix on the branch, re-verify,
then propose.

Acceptance: Stage-A section of the p5-10 review file complete; merge
landed on main with owner approval.

## Stage 2 — Fable, dev box: repair what the audit found

Precondition: Stage 1 merged (fixes build on main, on top of phase 5).

1. **List refresh — conditional GET, mandated.**
   `If-None-Match`/`If-Modified-Since` with correct 304 handling is the
   primary mechanism — it eliminates the *download*, which is the actual
   cost (12–23 MiB RSS excursions + ~20 MB/day bandwidth). Hash-compare
   before parse is a **fallback only** for origins that serve no
   validators — it is not an equivalent alternative, because it still pays
   the download. Persist validators beside the cached `.raw` files;
   a 304 refresh must allocate O(1), touch no list buffer, and not
   recompile. Streaming parse is out of scope for this pass.
2. **Observability.** Export `lists.bytes_fetched` (per refresh outcome:
   200-with-body vs 304) and add the allocator-commit figure to the perf
   sample, so a future soak attributes any excursion from `/history/perf`
   alone. The 2026-08-29 hunt only closed because the owner opened the
   router's bandwidth graph — that dependency must not repeat.
3. **gen.py (F1).** Apply the proven Bresenham fix — reference copy at
   `docs/code-review/phase2.6/p2.6-11-session/audit-repro/gen-fixed.py`
   (wire-proofed: exact split, full coverage, `gen-fixed-run.json`) — to
   `p2.6-11-session/tools/gen.py`.
4. Quality gates (`fmt`, `clippy -D warnings`, full tests) green; hot path
   untouched by 1–2 (refresh is background); doc edits (CONFIGURATION.md /
   API.md for the new counter/field) proposed and owner-approved in the
   same change.

Acceptance: 304 path proven by test (mock origin returning validators);
counters visible in `/api/v1/telemetry` and the perf sample; gen.py fixed;
gates green; commits owner-approved.

## Stage 3 — Fable, dev box: re-run L.1s for real

Precondition: Stage 2 (fixed gen.py). **A real run, not a simulation** —
the simulated proof already exists and is not acceptance.

Re-run the L.1s protocol from
[p2.6-11-optin-deploy-soak-review.md](../docs/code-review/phase2.6/p2.6-11-optin-deploy-soak-review.md)
(frozen T0/T1 repetition discipline, declared design honored or amended
*in writing before running*). Strict acceptance, measured not asserted:

- forward/control split **500/1 000 QPS** from the T0/T1 counter deltas;
- **all 6 000 forward names touched** — cold-cache first-touch misses
  ≥ 6 000 (+ control set) on a restarted engine;
- declared cadence arithmetic holds (each name re-queried every 12 s;
  TTL 5 s ⇒ every post-first hit serves stale);
- SWR gates re-evaluated at the true declared rate
  (`swr.dropped == 0`, attempts identity, penalties/probes zero);
- results recorded in a new section of the task's review file, superseding
  the F1-tainted run.

## Stage 4 — Opus: build, pre-declare, deploy, re-soak 7 days

Precondition: Stages 1–3 complete on main.

1. **Build** the ARM64 image from main (phase 5 + Stage-2 fixes), version
   bumped; record image digest.
2. **Write the re-soak pre-declaration BEFORE deploy** (new file under
   `docs/code-review/`, committed with a timestamp before the deploy):
   - per-window half-to-half RSS drift < 2 MB — **all windows must pass**
     (the aggregation rule F2 found missing, declared this time);
   - **floor-plateau gate**: `floor(W7) − floor(W4) < 2 MiB` — a gate now,
     not report-only;
   - every `peak_rss` step attributed via `lists.bytes_fetched` +
     allocator-commit — an unattributed step **fails** (p2.5's criterion,
     restored);
   - `events_dropped == 0`, `swr.dropped == 0`;
   - list refreshes must show 304/no-body on unchanged lists — the
     Stage-2 fix proving itself in production.
3. **One router intervention, owner-run**, commands proposed with effects:
   Stage-0-style final pull (if not already current) → deploy new image as
   the serving container → p2.6 cleanup (old container removal, comment
   swap) → start. p5-10 **Stage B measurements at soak start**: three RSS
   readings, Argon2id cost, polled-endpoint costs, `constants.ts`
   refresh-default corrections, certificate re-check (scope in the p5-10
   task file).
4. **Re-soak 7 days**, read-only pulls only, every pull's raw JSON
   committed as it lands. Day-7 acceptance evaluates the declared gates —
   this soak, on the repaired code, carries the `adaptive` acceptance the
   terminated one no longer can, and closes p5-10 Stage B / phase 5.

## Open items this plan deliberately leaves with the owner

- The yes/no on each commit, the merge, every `.md` edit outside review
  files, and the Stage-4 command execution.
- F3's doc-level ruling (p2.5 criteria restored as gates here — record the
  decision in the re-soak declaration).
- Whether the terminated soak's review file gets its closing section as
  proposed in Stage 0.4.
