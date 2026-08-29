# Phase 2.6 Audit — p2.6-11 Opt-in Deploy and Soak (read-only)

**Audited:** [p2.6-11-optin-deploy-soak-review.md](p2.6-11-optin-deploy-soak-review.md)
at working-tree state, soak day 5 of 7 (2026-08-29).
**Method:** every gated figure recomputed from the committed raw data in
[p2.6-11-session/](p2.6-11-session/) and
[phase2.5/s1g4-window/](../phase2.5/s1g4-window/); generator and collector
tools read line by line; soak-section figures checked for internal consistency
only (their raw JSON is not committed — see F7). No code, config or review
file was modified.

## Summary

- M.8, L.1/L.2 tier 2 + tier 3, L.4a, L.4b and S1-G4 segment tables all
  **recompute exactly** from the committed raw data — estimates, deltas, CI,
  control margin, jitter windows, ladder sums, first-penalty transitions.
- **F1: L.1s did not run its declared workload.** A rounding bug in the
  committed generator halves the split to 750/750 QPS and reaches only
  3 000 of the 6 000 declared SWR names. The raw data proves it; the review
  never noticed. Verdicts survive, arithmetic and reproducibility do not.
- **F2: the declared soak gate has already failed** (W3, +5.702 MiB) and has
  no declared rule for what a failed window means; its sensitivity to
  slow ratchets is near zero by construction.
- **F3: two of p2.5-09's four soak criteria were dropped** — residual
  monotone-drift and peak-step attribution — and both are exactly the
  criteria the observed soak data would stress.
- Several internal contradictions (S1-G4 run count and rate, L.1s identity
  claim, L.1s declared-vs-executed design) — none reverses a measured
  verdict, all are declaration-discipline defects in a file whose method is
  declaration discipline.

## What was recomputed and matched

| Section | Raw source | Result |
| --- | --- | --- |
| M.8 all 9 pairs, `d_i`, mean −0.2459 %, s 2.5581, CI [−2.385, +1.893], control −1.951 % | `m8/pairNN-*.out` | exact match, all three clauses PASS as reported |
| L.1/L.2 attempts/failures/penalties/probes deltas, all 6 reps + warm-up | `l1l2/repNN-*/t{0,1}-telemetry.json` | exact match; tier 3 delta +0.0359 % confirmed |
| L.4a counters 16/16/15/14, `penalized_seconds_total` 3 660 = 24+48+96+192+300×11 | `l4/l4a/` t0/t1 | exact; ladder sum closes to the second |
| L.4a/L.4b slow-query counts (16, 14), window gaps, jitter ratios | `slow.jsonl` | exact match to both window tables |
| L.4a first penalty after exactly 2 attempts at offset 6.26 s; L.4b sample 0 already `penalized 1/2/2` | `first60.jsonl` (240 × 250 ms both arms) | confirms arming narrative and the pre-run-check placement |
| L.4b measured-window delta 14/14/14/14 | `l4/l4b/` t0/t1 | exact |
| S1-G4 segments 18 780/1, 12 287/1, 11 572/1, `failure_runs [1,0,0,0]` each | `s1g4-window/seg0*.json` | exact — **3** closed runs (see F6) |
| L.4a client p50/p99/max | `rep01-L4A/load-stdout.txt` | committed, matches |
| 0.2.20 code-identical to probe `f65386f` | `git log f65386f..1c430aa` | holds — 5 docs commits + the version bump, no code (review says "one documentation commit"; trivial) |

Positive controls exist for the counter gates: L.4a/b prove `penalties`,
`probes`, `failures` and the attempts-identity excess all move on this build
when an endpoint is dead, and the L.1s warm-up's 410 drops prove
`swr.dropped` can move. The soak's zeros are therefore meaningful, and W3
proves the RSS gate can fire. The gates are falsifiable; F2 is about *what*
they can detect, not whether they fire.

## Findings

### F1 — MAJOR: L.1s never ran its declared workload; the generator halves the split and the name set

The declared workload — 500 QPS over a recurring 6 000-name set + 1 000 QPS
control — is not what [gen.py](p2.6-11-session/tools/gen.py) produces at
those arguments:

- `ctl_every = max(1, round(1500/1000))` = `round(1.5)` = **2** (banker's
  rounding). Every second query is control: **750 QPS control, 750 QPS
  forward**, not 1 000/500.
- Forward names are drawn as `fwd[n % len(fwd)]` with the shared counter
  `n`, and every even `n` is control. With `len(fwd) = 6000` (even), forward
  queries only ever see **odd indices — 3 000 of the 6 000 names**. Control
  likewise uses only the even half of the 100-name control set.

The raw data confirms this independently, three ways:

| Evidence | Declared model predicts | Bug model predicts | Measured |
| --- | --- | --- | --- |
| Cold-cache first-touch misses (calibration `C`; rep 4 after restart) | ≥ 6 000 | 3 000 fwd + 50 ctl ≈ 3 050 | **3 002** (`C`), **3 050** (rep 4) |
| Steady refresh rate (3 000 names, 4 s re-query, TTL 5 s ⇒ stale every other pass ⇒ 375/s) | ≈ 500/s | ≈ 375/s | **350/s** (`C`), **380/s** (reps) |
| Total client rate | 450 000 / 300 s | same (total unchanged) | 450 002 ✓ |

The review reports "380 refreshes/s sustained" and C's 350/s next to its own
≈ 500/s prediction without reconciling them — the visible symptom went
unread. Consequences:

- The declared arithmetic ("each name re-queried every 12 s, past expiry, so
  every hit after the first serves stale") describes a workload that never
  ran. The actual cadence is 4 s re-query with TTL 5 s, so only every
  *second* pass serves stale.
- The `swr.dropped == 0` gate was exercised at ~76 % of the declared enqueue
  rate; the worker-saturation margin is smaller than the file implies.
- Reproducing "the declared workload" from this file reproduces the bug,
  silently, on any even-sized name file with `ctl_every = 2`.
- L.1/L.2 and p2.6-10 are unaffected: at 10 000+1 000 QPS,
  `ctl_every = 11` and `gcd(11, len) = 1`, so both rates and full name
  coverage hold there.

What survives: SWR **was** exercised at load (111 000–114 000 refreshes per
repetition), and the failed/dropped/penalty/probe zeros are real. The PASS
rows stand as evidence about *a* sustained SWR workload — a weaker and
different one than declared. The section's workload description, calibration
model, and the "6 000-name set" claim do not stand.

### F2 — MAJOR: the declared soak gate has already failed at W3 and carries no aggregation rule; its detection power over the claimed failure mode is narrow

- W3 is complete (240 samples) and its drift, **+5.702 MiB**, exceeds the
  declared < 2 MB. No later data changes W3's rows. The plan and review both
  say the half-to-half drift "is the one pass/fail" but neither declares
  what a failed window means for the 7-day verdict — all-must-pass (soak
  already failed), or something laxer (undeclared). "Interim decides
  nothing" is true only of unfinished windows; deferring the W3 consequence
  to day 7 without a declared aggregation rule leaves the verdict to be
  decided after the results are known — the exact failure mode this file's
  pre-declaration method exists to prevent.
- Sensitivity: the metric compares the means of hours 16–20 vs 20–24 of
  each day. It detects growth expressed *inside the final third* faster than
  ~0.5 MiB/h. A ratchet that accrues during daytime traffic and plateaus by
  hour 16 passes every window regardless of size; over 7 days a +5 MiB/day
  daytime ratchet (+35 MiB) is invisible to the gate. The observed floor
  series — the discriminator for exactly that shape — rose monotonically in
  all four windows (+9.94, +2.98, +0.53 MiB) and is report-only.
- The retraction in the review overcorrects: W3 proves the gate can catch a
  final-third excursion; it does not prove the gate catches day-scale
  ratchets, which was the substance of the retracted objection.

### F3 — MAJOR: p2.5-09's soak invariants were weakened, and the dropped ones are the two the data stresses

[p2.5-09 V6](../phase2.5/p2.5-09-phase-verification-review.md) gated four
criteria. L.3 keeps none of them as gates:

| p2.5-09 criterion | L.3 status | What the p2.6-11 data shows |
| --- | --- | --- |
| Residual: thirds slopes must not share a sign | dropped (residual report-only) | `residual_bytes` rose monotonically across pulls 1–3, +20.86 MiB, before the pull-4 fall |
| `peak_rss` steps only at a list refresh, each step attributed | dropped entirely | peak stepped 137.17 → 142.80 → 142.83 → **150.61 MiB** across pulls, **no step attributed to anything** |
| `events_dropped` delta 0 | report-only | 0 — no impact |
| `swr.dropped` delta 0 | report-only | 0 — no impact |

Under p2.5's rule an unattributed peak step "fails — it does not matter that
RSS fell back afterwards". The soak shows +13.44 MiB of unattributed peak
movement and the review's only comment is that peak "exceeds RSS by design".
Nothing in the file records that these criteria were consciously retired;
they were silently not carried. A doc-level decision (keep, restate, or
retire with a reason) is owed either way.

### F4 — MODERATE: L.1s executed design contradicts its own pre-declaration, unrecorded

Declared: "`adaptive` and `fallback`, **alternating**, same session, **no
restart between repetitions**." Executed (confirmed from raw `meta.json`
uptimes): blocked — calibration + warm-up + 3 × `fallback` in one process
(uptime 24 409 → 26 179 s), then a flip **with restart** (rep 4 t0 uptime
**116 s**), then 3 × `adaptive`. The results text narrates the blocked
design openly but never flags it as a deviation, and no amendment was
recorded — in a file that twice states "a declaration that can be quietly
edited is not a declaration", executing a different design without recording
the change is the same defect in mirror image. (L.1/L.2's blocked design,
by contrast, was declared as such up front.)

### F5 — MODERATE: the L.1s attempts-identity claim is contradicted by the raw data, and four transport failures went unreported

Raw deltas: rep 1 (`fallback`) has Σ attempts **114 001** against
`swr.completed` 114 000 with `failures = [1, 0]` and 0 misses; the warm-up
has Σ 113 593, completed 113 590, `failures = [3, 0]`. The review's claim —
"the stronger equality `attempts == swr.completed + swr.failed`, exactly, in
those five" — is **false for rep 1** (off by exactly the one failed attempt,
the same excess-equals-failures reading the review itself establishes for
L.4). The declared per-repetition reporting includes `upstreams[].attempts`
per endpoint and would have surfaced this; the results table aggregates it
away. The L.1/L.2 section reports its two analogous single failures
prominently; the L.1s section hides four. The acceptance verdict is
unchanged in substance, but a gate row ("Attempts identity — PASS") is
currently justified by a sentence the raw data contradicts.

### F6 — MODERATE: S1-G4 prose contradicts its own table — 2 vs 3 closed runs, 0.0050 % vs 0.0070 %

The segment table and the raw seg files agree: **3** closed runs (one per
process), 3 failures / 42 639 attempts = **0.0070 %**. The decision text
says "closes the S1-G4 window permanently at **2** closed runs" (twice) and
uses **0.0050 %** (twice more, including S1-G5's "order of magnitude
quieter" and the extension sizing). The prose predates segment 03's third
failure and was not updated. Knock-on: at the real rate (~1 failure /
14.8 h) the 15–20-closed-runs extension is ~9–12 days, not 12–17. No
verdict changes — S1-G4 remains not validated either way — but the file
currently states two different measurements for the same quantity.

### F7 — MODERATE: "Every figure in this file is recomputable from p2.6-11-session without the appliance" is false for the soak section

The interim pulls' raw JSON is explicitly not committed ("the final pull
supersedes it"). Every W1–W4 figure — the failed W3 gate value, floors,
ceilings, the residual/accounted table, the peak series — is unverifiable
from the repository today. The final pull will contain the full 7-day row
set (360 s cadence, 30-day retention), so the gap closes *if* that pull is
committed; until then the soak section, including its one FAIL, rests
entirely on transcription. The recomputability claim should be scoped to
the probe/dev-box arms, or the interim JSON committed.

### F8 — MINOR: pull 2 is internally inconsistent — its perf rows end 6.4 h before its own uptime timestamp

`uptime_seconds` 167 094 places pull 2 at 2026-08-27T06:21:56Z; its rows end
2026-08-26T23:57:40Z — 64 missing rows at the 360 s cadence. Pulls 1, 3, 4
end within 110–273 s of their pull instants. Either the perf snapshot was
truncated (401 rows; the API default `max_points` is 1 000, so not the cap)
or the uptime was read at a different instant than the perf pull. Evidence
is unaffected — pull 3's 696 continuous rows cover W2 — but the row as
printed cannot describe one instant, and the no-restart argument should
cite the uptime deltas (which do match the inter-pull gaps) rather than
this row's face value.

### F9 — MINOR: the peak-RSS series is the only witness of unsampled excursions and goes unread

Sampled ceilings never exceed 81.52 MiB; the process high-water mark reached
150.61 MiB. Something repeatedly runs ~70 MiB above what the 360 s sampler
sees — the boot-compile transient explains the initial 137 MiB, and list
refreshes are the obvious suspect for the later steps (that was p2.5's
criterion 4, F3), but the review draws no inference at all. On a 1 GiB
device shared with RouterOS, transient footprint is the OOM-relevant figure
and no gate or report row watches it.

### F10 — MINOR: L.1's "forward names are unique and never re-queried" is wrong as stated

The 300 000-name file is cycled ~20× per 600 s repetition (6 M forward
queries), so every name is re-queried every ~30 s. Zero SWR holds because
the 10 000-entry cache evicts each name within ~1 s of insertion (evictions
delta ≈ misses ≈ 6 M per rep in the raw t0/t1), not because names are never
re-queried. The conclusion is right; the stated mechanism is not — and an
asserted-but-unexamined workload property is exactly what let F1 through.

### F11 — MINOR / question: the 0.2.12 cache invariant does not close on the pull-1 numbers as printed

p2.5 carried `hits + misses == pass + allow`. Pull 1: 39 116 + 3 295 =
42 411 vs `pass` 42 374, `allow` 0 — off by 37 in a snapshot where average
load (~1.6 QPS) cannot explain 37 in-flight queries. Possibly a
counter-semantics change under stale/SWR accounting, possibly non-atomic
snapshot skew accumulated elsewhere. Worth one line of attention at the
final pull; the invariant is currently neither asserted nor satisfied.

## Cross-checks with no finding

- **Restart detection:** uptime, peak, attempts, swr and `failure_runs` are
  all monotone across pulls; uptime deltas equal inter-pull gaps. No missed
  or false restart. Segment 02's lost tail is honestly recorded.
- **Silent state transitions:** none. `state` healthy / `penalties` 0 at
  every pull is consistent with 8 isolated failures under
  `penalty_failures = 2` (consecutive); `9.9.9.9` attempts == `1.1.1.1`
  failures at every pull — the 1:1 fallthrough — and post-flip attempt
  rate (~622/h) matches the pre-flip rate (929/h) times the spec-mandated
  `resolve_host` exclusion (36.5 %).
- **Cache vs residual:** `cache_estimated_bytes` moves within ±1.7 MiB and
  entry counts fall via cleanup, against residual swings of ±21 MiB — the
  review's "movement is entirely residual" attribution is arithmetically
  right (`residual = rss − accounted` verified at all four pulls).
- **Traffic vs memory:** window query counts fall monotonically while floors
  rise — the review's "not load-driven" reading is supported by its own
  table (raw unverifiable, F7).
- **Partial windows:** W4 correctly carries no gate figure; pull 1's
  premature W1 figure was superseded and both values retained.
- **Provenance:** probe arms self-report 0.2.19/`f65386f` as disclosed;
  production runs the `1c430aa` artefact, pinned by `image-id` = the OCI
  config blob digest; cadence 360 s matches the live config.

## Verdict

The measured arms are solid: every gated number in M.8, L.1/L.2, L.1s
(counters), L.4a/b and S1-G4 recomputes exactly from committed raw data, and
the honest-limit discipline (L.4b UNCONFIRMED, S1-G4 not validated, thin
control margin) is real, not rhetorical. The defects cluster in the
*declared-workload and declared-gate layer*: one workload that never ran as
declared (F1), one gate that has already fired without a declared
consequence (F2), two silently dropped prior-phase invariants (F3), and a
set of unrecorded deviations and stale prose (F4–F6). None of these is
repaired by editing history; each is repaired by recording it — which is
this file's own standard.

**AUDIT: FINDINGS AS LISTED — F1–F3 need an owner ruling before the soak's
day-7 acceptance is written; F4–F11 are recordable as-is.**

## F2 ruling — owner-directed, written 2026-08-29, before W4's final third, W5, W6 or W7 have been read

What can still be pre-declared is declared here; what cannot is named as
already lost. Nothing below is revised after the day-7 pull.

1. **W3 is a FAIL of the declared gate and stays one.** The gate as declared
   ("the one pass/fail", per window, < 2 MB) was seen and applied; W3
   exceeded it at +5.702 MiB. No aggregation rule written today can be
   *pre*-declared with respect to W3, because today is after W3. It is not
   reinterpreted, re-windowed, or excused.
2. **Overall declared-gate verdict at day 7: FAIL if any window fails.**
   W3 has failed, so the L.3 gate verdict is already **FAIL** and the day-7
   write-up records it as such. The soak cannot be cited as passing evidence
   for Stage-1 acceptance.
3. **The remaining evidence classifies the excursion; the rule is fixed
   now.** W4–W7 are evaluated under the gate exactly as declared. The W3
   excursion is recorded as **transient** only if both hold: (a) W4, W5, W6
   and W7 all pass the < 2 MB rule; (b) the daily RSS floor plateaus —
   `floor(W7) − floor(W4) < 2 MiB`. Any other outcome records the soak as
   **ratchet-suspect**, and the correct consequence named in advance is a
   further soak (or non-ship), not a re-reading of this one.
4. **Consequence is an ops decision, taken openly.** Whether `fah-next`
   keeps running `adaptive`, the soak is extended, or the container is
   rolled back is decided by the owner at day 7 and recorded as a decision
   in the face of a failed gate — the same form as the S1-G4 flip ruling —
   never as a pass.
5. This ruling gains its pre-declaration force only from a commit timestamp
   before 2026-09-01T07:57Z.
