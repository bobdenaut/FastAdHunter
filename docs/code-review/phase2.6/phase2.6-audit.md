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

**Fix, proven off-repo (2026-08-29; repo `gen.py` untouched pending owner
ruling).** Replace the shared-counter `ctl_every` modulo with a Bresenham
interleave and independent per-stream counters (`ctl_acc += ctl_qps; if
ctl_acc >= total: ctl_acc -= total → ctl[n_ctl++]` else `fwd[n_fwd++]`) —
exact control fraction for any integer ratio, full coverage of both name
sets at any set size. Two proofs, patched copy at the session scratchpad
(`gen-fixed.py`, same dir as the raw pulls named under F11):

| Proof | Old | Fixed |
| --- | --- | --- |
| Simulated 450 000 sends at L.1s args (500+1 000, 6 000/100 names) | 750/750 QPS, 3 000 fwd + 50 ctl names | **500/1 000 QPS exactly; all 6 000 fwd (25× each = 12 s cadence, the declared arithmetic); all 100 ctl (3 000× each)** |
| Simulated 6.6 M sends at L.1/L.2 args (10 000+1 000, 300 000/100) | correct | identical — confirms those runs unaffected |

Live wire smoke (patched file, real UDP against a local sink, 150 QPS × 12 s,
60/10-name sets): 1 800 sent at 150.0 achieved QPS; sink decoded 600 fwd
(50 QPS, all 60 names, exactly 10× each) + 1 200 ctl (100 QPS, all 10
names, exactly 120× each). Zero send errors. The declared L.1s workload is
producible; re-running L.1s with the fixed generator remains the owner's
F1 ruling.

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
**Retracted — see the F11 resolution section at the end of this file: the
audit assumed `allow 0` where the raw value is 37; the invariant holds
exactly at all four pulls.**

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

## F11 resolution — RETRACTED, no bug (2026-08-29, read-only follow-up)

The invariant holds exactly. The audit's pull-1 `allow 0` was an assumption,
not a measurement — the review prints pass/block/hits/misses/stale but never
`allow` (F7's uncommitted raw made the gap invisible). The interim pulls' raw
telemetry JSON was found intact in a prior session's scratchpad and recomputed:

| Pull | uptime s | pass | allow | hits + misses | pass + allow | diff |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 84 748 | 42 374 | **37** | 39 116 + 3 295 = 42 411 | 42 411 | 0 |
| 2 | 167 094 | 68 528 | 132 | 64 072 + 4 588 = 68 660 | 68 660 | 0 |
| 3 | 250 511 | 89 042 | 138 | 82 408 + 6 772 = 89 180 | 89 180 | 0 |
| 4 | 315 123 | 95 108 | 138 | 88 044 + 7 202 = 95 246 | 95 246 | 0 |

Code corroborates: `Metrics::record`
([registry.rs:128–152](../../../crates/fah-metrics/src/registry.rs)) is the
only increment site for both counter families; the verdict match is
exhaustive, the cache branch excludes `Block` symmetrically, so every
non-blocked event increments exactly one of hits/misses and exactly one of
pass/allow. The event fan-out is a single consumer (`spawn_event_fanout`),
so snapshot skew is bounded by one in-flight event. `fah-metrics` and
`fah-dns` are byte-identical between the deployed `1c430aa` and HEAD.

Knock-on facts:

- The soaking appliance serves real `Allow` verdicts (37 → 138 across pulls)
  — the review's counter table omits the column entirely; worth adding at
  the day-7 write-up.
- F7's cost is now demonstrated twice: the missing raw made the review
  unverifiable *and* induced a false finding in the audit itself. The four
  interim telemetry pulls (2.5 KB each) and perf pulls exist at
  `%LOCALAPPDATA%\Temp\claude\e--FastAdHunter\06364f7b-…\scratchpad\` —
  ephemeral location; committing them into `p2.6-11-session/soak/` at day 7
  closes F7 for the whole soak, not only the final pull.
- Live cross-check during this follow-up: `/health` reports 0.2.20,
  uptime 319 524 s — still monotone, no restart.

## Perf-series follow-up — F9 attribution closed, W3 mechanism identified (2026-08-29, read-only)

Source: the pull-4 `/history/perf` raw JSON (same scratchpad as above) —
876 rows at 360 s covering boot 2026-08-25T07:57:05Z through
2026-08-28T23:27:40Z, the full soak to date in one series. All figures below
recompute from it; the review's transcribed W1–W3 floors (+9.94, +2.98,
+0.53 MiB), ceilings and the W3 drift **+5.702 MiB** match the raw exactly,
so F7 narrows to "raw uncommitted", not "figures wrong".

### List-refresh cadence, witnessed

`ruleset_bytes` changes at exactly two sample instants per day —
**10:39:40Z and 22:39:40Z** (12 h cadence; first tick boot + 2h42m) — each
with a minor-page-fault spike (38 k–205 k vs ~1 k background). These are the
recompiles.

### Every peak-RSS step lands on a refresh tick — p2.5 criterion satisfied 5/5

| Instant | peak_rss step | Refresh witness at same row |
| --- | --- | --- |
| 08-25T10:39:40 | 94.09 → 133.66 (+39.57) | ruleset −0.8 KiB, 38 518 faults |
| 08-25T22:39:40 | → 137.17 (+3.51) | +5.6 KiB, 103 512 faults |
| 08-26T22:39:40 | → 142.80 (+5.63) | −62.2 KiB, 192 215 faults |
| 08-27T22:39:40 | → 142.83 (+0.03) | +27.2 KiB, 101 074 faults |
| 08-28T10:39:40 | → 150.61 (+7.78) | +2.4 KiB, 47 275 faults |

No peak movement occurs anywhere else. The p2.5-09 criterion F3 said was
dropped — "peak steps only at a list refresh, each step attributed" — turns
out to **hold** on this soak; it was satisfied, not violated, just never
checked. F9's ~150 MiB "invisible transient" is the recompile
(double-buffered ~25 MiB ruleset + parse/download buffers), the same
mechanism as the boot transient. F9 and the F3 peak-attribution half are
closed; the F3 governance point (criteria silently dropped) stands.

### W3's failed gate: one decaying anonymous excursion, not a ratchet

The RSS series is a sawtooth: irregular anon-only jumps of +12–23 MiB that
decay back over 0.5–5 h, on flat cache, flat ruleset, normal traffic, and
**not** on refresh ticks — 08-26T13:09 (+16.6), 08-26T14:45 (+14.1),
08-27T19:21 (+23.1), 08-28T05:21 (+12.9), 08-28T15:33 (+12.3 MiB residual).

W3's +5.702 MiB half-to-half drift decomposes exactly: the 08-28T05:21:40
excursion (60.93 → 76.81 MiB, decayed only to ~67 by window end) sits in the
final third's second half; excluding its 26 elevated samples the drift is
**−0.121 MiB**. The gate fired on one transient landing in its comparison
half — the shape it is built to catch — not on monotone growth.

Knock-ons:

- **F3's "+20.86 MiB monotone residual across pulls 1–3" is pull-instant
  aliasing.** Pull 3 (boot + 250 511 s = 08-28T05:32) landed 11 minutes
  after the 05:21 excursion peak. The underlying series is a sawtooth
  around a rising-but-decelerating floor, not monotone growth.
- **Floor series decelerates**: +9.94, +2.98, +0.53 MiB — consistent with
  allocator retention (mimalloc v3) approaching a plateau, not a linear
  ratchet. If it continues, the F2 ruling's transient-classification
  condition `floor(W7) − floor(W4) < 2 MiB` is plausible. Stated as a
  prediction, falsifiable at day 7; the W3 FAIL and the ruling are
  unchanged by any of this.
- **Open**: the excursions' allocator-level cause (what allocates 12–23 MiB
  anon off-refresh at ~1 QPS, released over hours) is not identifiable from
  this series. Candidates worth one look at day 7: SWR burst + mimalloc
  segment retention; stats rollup. Sizes and decay argue allocator arena,
  not a leak.

## F8 resolution — stale `to` parameter on pull 2, no truncation, no sampler gap (2026-08-29, read-only)

The raw pull files carry the request windows. Pull 1 and pull 2 were both
issued with **`to=2026-08-27T00:00:00Z`**; pull 2 (taken 06:21:56Z per its
uptime) simply reused pull 1's query window, so its rows stop at
2026-08-26T23:57:40Z — the last sample before its own `to` bound. Not the
`max_points` cap (401 < 1 000), not a sampler stall, not a snapshot skew:
an operator-side stale parameter.

| Pull | requested `to` | rows | last row | pull instant | gap explained |
| --- | --- | --- | --- | --- | --- |
| 1 | 08-27T00:00 | 236 | 08-26T07:27:40 | 08-26T07:29 | natural (2 min) |
| 2 | 08-27T00:00 | 401 | 08-26T23:57:40 | 08-27T06:21 | **`to` bound** |
| 3 | 08-29T00:00 | 696 | 08-28T05:27:40 | 08-28T05:32 | natural (5 min) |
| 4 | 08-30T00:00 | 876 | 08-28T23:27:40 | 08-28T23:29 | natural (2 min) |

The 64 rows pull 2 did not fetch exist: pull 3 covers 08-26T23:03 →
08-27T09:57 with a maximum inter-row gap of exactly 360 s. No data was ever
missing; the review's no-restart argument needs no repair beyond citing
uptime deltas, as F8 already recommended.

## Excursion cause hunt — every exported counter ruled out; unattributable from outside (2026-08-29, read-only)

Twelve single-sample RSS jumps ≥ 6 MiB over the soak (6.2–23.1 MiB). What
the raw series and the pull counters exclude:

- **Not scheduled.** Onset times are aperiodic: inter-arrival 0.4–20.7 h;
  offsets modulo 1 h and modulo 12 h are scattered. Rules out the perf
  sampler (360 s), cache cleanup (360 s — 875 runs, µs-scale, 4 MB freed
  *total*), snapshot/history flush (fixed cadence), and list refresh (12 h
  ticks; exactly one excursion, 08-26T22:39, coincides — the rest miss both
  daily ticks).
- **Not traffic.** Onset-row qps 0.7–2.8 at seven of nine major onsets (the
  two others 2.8/10.5); `queries_delta`, hit/miss deltas, forward p99 all
  unremarkable; the largest excursion (+23.1 MiB, 08-27T19:21) sits at
  2.6 qps.
- **Not the cache.** Zero evictions at every onset; `cache_estimated_bytes`
  flat; entry counts flat.
- **Not SWR.** Production SWR averages ~0.15 refreshes/s (47 233 completed
  over 3.6 days), `dropped = 0`, `failed = 0` — three orders of magnitude
  below the dev-box arms that stayed flat.
- **Not upstreams.** No TLS handshakes (plain UDP), attempt deltas normal,
  no failures at onsets.

What the data does say: each onset row carries a minor-page-fault burst of
~3 000–7 500 pages — roughly the jump size at 4 KiB/page — so something
touches 12–23 MiB of fresh anonymous memory once, inside one 360 s
interval, then frees it; RSS decays back over 0.5–5 h in 2–5 MiB steps.
That profile — one-shot commit, slow stepped release — matches mimalloc v3
segment commit followed by delayed purge, triggered by a transient
allocation that no exported counter measures.

~~Terminus: unattributable from outside the process.~~ **Superseded the
same day — cause found and reproduced on demand; see the next section.**

For the F2 ruling nothing changes: the excursions are transient by
observation (they decay), the floor series decelerates, and the
classification rule already written covers both outcomes.

## Excursion cause — FOUND: scheduled list re-downloads, buffered whole in RAM (2026-08-29, dev-box repro)

**Mechanism.** Every list refresh re-downloads the full body — there is no
conditional GET (no ETag/If-Modified-Since anywhere in
`fah-rules/src/lifecycle/`) — and accumulates it in a growing `Vec<u8>`
([source.rs:64–85](../../../crates/fah-rules/src/lifecycle/source.rs)),
converts to `String`, parses, and writes `/data/lists/*.raw`. Per-list
schedules are seeded from cache-file mtimes
(`lifecycle/mod.rs`), so the 48 h lists fire at phases scattered across the
day — the aperiodicity that ruled out every fixed-cadence suspect. A
download whose content is unchanged does **not** recompile (no
`ruleset_bytes` witness, no peak step); only the frequently-changing lists
(`phishdestroy` 12 h, `tif-mini` 24 h) land recompiles on the 10:39/22:39
grid. The router's own `veth1` graph corroborates: inbound spikes of
~8–20 MB at excursion-shaped times against a ~17 Kb/s baseline.

**Reproduced on demand** (dev box, x86_64 build of `1c430aa`, identical
config/lists, strace + 1 s `smaps_rollup` sidecar): `POST
/api/v1/lists/refresh` produced within seconds RSS jumps of **+16.5 MiB**
(download buffers), **+23.1 MiB** (further downloads — equal to the largest
production excursion), then **+116.7 MiB** (the 16-list recompile — the
production peak-step/boot-transient scale). Full-smaps diffs put **all** of
the growth in a single 1 GiB anonymous mapping — mimalloc v3's arena — i.e.
pure in-heap allocation, no thread stacks, no kernel-side surprise. The
slow 0.5–5 h decay is mimalloc returning committed arena pages.

**Judgement.** Not a leak — bounded transients from a by-design (if
uneconomical) full re-download path. Improvement candidates, each a
post-soak decision, not a soak repair: conditional GET (ETag), streaming
parse instead of whole-body buffering, or hash-compare before parse. On the
RB5009's 1 GiB the transient coexists with the 128 MB budget only because
the floors stay low; a list growing to 30 MB would push the recompile
transient proportionally.

**Observability lesson.** The counter hunt ruled out every exported
observable and stalled precisely because the refresh download path exports
none — the breakthrough witness was the router's *bandwidth* graph
(owner-supplied), not the process. Whatever fix is chosen should export a
counter for the next hunt to find — `lists.bytes_fetched` (and ideally the
allocator-commit figure in the perf sample), so a memory excursion can be
correlated with its cause from `/history/perf` alone, without needing
someone to think of opening the interface graph.

Raw evidence in the session scratchpad (`repro/out/`): `smaps.log` (1 s),
`smaps-full-*/maps-*` dumps at each jump, `strace.log`, `memwatch-repro.jsonl`.

## Device-level memory creep — day-7 check item (2026-08-29)

RouterOS graphs show device "used" creeping ~260 → ~288 MiB since the
Tuesday deploy; the daily graph is already flat (286–304 MiB band, ~28 % of
1 GiB — no OOM trajectory). Expected decomposition, to be verified at the
day-7 close with `/system/resource/print` free-memory against container
RSS across two pulls:

| Term | Size | Basis |
| --- | --- | --- |
| fah RSS floor climb | ~13 MiB, decelerating | audited floor series (perf-series follow-up above) |
| Blocklist files on `/data` + their page cache | ~27 MiB, rewritten per refresh | measured on the dev-box repro of the same build/config |
| Perf history file | 1 621 B/row = 380 KiB/day, caps at ~11 MiB at 30-day retention (~1.5 MiB by day 4) | measured on the repro |
| Remainder | RouterOS internals, container layer | unattributed |

Flag only if device "used" keeps climbing after the fah floor has
plateaued and faster than the ~0.4 MiB/day history growth.

## Soak termination — owner decision, 2026-08-29 (day ~5 of 7): NOT a PASS

The owner terminated the L.3 soak early: the excursion cause is found,
reproduced and judged benign (section above), so days 6–7 add observation,
not information. Recorded per the F2 ruling's point 4 — an ops decision
taken openly in the face of a failed gate, never a pass. The plan forward
is [plan/resoak-orchestration.md](../../../plan/resoak-orchestration.md):
fix (conditional GET + observability), then a 7-day re-soak on the
repaired build under pre-declared, stricter gates. That re-soak — not this
soak — carries the `adaptive` acceptance.

Final state, from the final read-only pull
([soak/pull5-final-*](p2.6-11-session/soak/), uptime 362 660 s, no restart
ever, `0.2.20` throughout; committed raw covers the full series, closing
F7 completely):

| Window | n | floor MiB | ceil MiB | half-to-half drift | Gate |
| --- | --- | --- | --- | --- | --- |
| W1 | 240 | 41.92 | 62.94 | −0.514 | pass |
| W2 | 240 | 51.86 | 71.32 | −0.547 | pass |
| W3 | 240 | 54.84 | 81.52 | **+5.702** | **FAIL — stands** |
| W4 | 240 | 55.37 | 75.06 | −2.067 | pass |
| W5 | 48 (partial at termination) | 54.31 | 60.56 | — | no figure |

- **W3 = FAIL stands** exactly as the F2 ruling fixed it. The declared-gate
  verdict for this soak is **FAIL**, and it is closed as
  **terminated-early / not-a-PASS**.
- The F2 classification (transient iff W4–W7 all pass and
  `floor(W7) − floor(W4) < 2 MiB`) can no longer complete — W5–W7 were
  never observed. What the data through termination shows: W4 passed, and
  the floor series 41.92 → 51.86 → 54.84 → 55.37 → 54.31 (partial) had
  plateaued and turned down. Combined with the reproduced benign cause,
  the excursion is *judged* transient by evidence outside the declared
  rule — recorded as a judgement, not as the rule's output.
- Counter gates at termination: `events_dropped` 0, `swr.dropped` 0,
  `penalties` 0, 8 isolated upstream failures (no run ≥ 2), and the cache
  invariant closes exactly (`hits+misses = pass+allow = 100 817`).
- `fah-next` keeps serving unchanged until the single Stage-4
  intervention (deploy of the repaired build + p2.6 cleanup).

## Re-soak termination — 0.3.0, owner decision 2026-09-01 (T0+~59 h of 7 d): no verdict

The owner terminated the `0.3.0` re-soak early to change the measurement
method. **This is not a gate outcome and is not a FAIL** — it is a
methodology change taken before day 7, and the soak therefore produces **no
declared-gate verdict at all**. Gates declared in
[resoak-0.3.0-predeclaration.md](resoak-0.3.0-predeclaration.md) are left
where they stood; that file is not edited, so its pre-declaration property
is preserved.

### Why terminated: the declared pull method drives the thing being measured

| | |
| --- | --- |
| Declared method | §Method — "**`?fields=` is never passed**", to keep `list_fetch`, `memory` and `allocator_committed_bytes` in the sample |
| Consequence | no `?fields=` ⇒ [`PerfFields::ALL`](../../../crates/fah-api/src/wire.rs) ⇒ `upstreams: true` ⇒ every gate pull parses the full `upstreams` array of every row in range |
| Mechanism | that parse is what `d420f38` identifies as the RSS ratchet — `upstreams` is 77.5 % of a row's bytes and nearly all of its per-row heap; mimalloc sizes retained arenas to the churn and holds them |
| Effect | every pull and every dashboard view injects an RSS excursion into the series being gated |

Measured on device from the final pull's full series, RSS MiB:

| Pull | −1 h | +0 h | +1 h | +2 h | +6 h |
| --- | --- | --- | --- | --- | --- |
| T0+45.4 h (dashboard open) | 65.3 | 83.7 | 80.6 | **64.8** | 64.6 |
| T0+58.6 h (gate pull alone) | 67.6 | 74.5 | 72.6 | **66.3** (h59.9) | — |

Series length compounds the excursion: 587 rows / 1.15 MB at T0+58.6 h,
~1 680 rows / ~3.3 MB by day 7.

**The excursions decay inside ~2 h.** They therefore distort G1 (which
compares hour-16–20 against hour-20–24 means) and can push a `peak_rss` step
under G3 — but they never set a 24 h minimum, so **G2's floor is not
observer-contaminated**. What makes the remaining 4½ days uninformative is
G1 and G3, not G2: with pulls unscheduled and dashboard sessions unlogged,
neither gate's figures could be separated from the act of reading them.

**The G2 floor ratchet is product-side and stands as a finding** — see
§State at termination.

### G5a — owner ruling: MOOT

**`counters.lists.not_modified > 0` within the first 48 h did not occur
(`not_modified` = 0 at every pull). The gate fails on the letter and the
ruling is MOOT — it does not apply to any final verdict, because this soak
was terminated for a methodology change and constitutes no acceptance
evidence.** Recorded explicitly rather than left implicit, per F3: a
criterion that stops applying must say so in writing.

Why the letter-failure carries no signal about the fix:

- `0.2.20` wrote no `.validators` files
  ([cache.rs:15](../../../crates/fah-rules/src/lifecycle/cache.rs)), so at
  `T0` every list's stored validator was absent and its first refresh was
  unconditional **by construction** — the predeclaration itself declares
  those 16 bodies unavoidable (§G5b).
- The first validator-armed 48 h wave was due 2026-09-01T22:35Z, i.e.
  **T0+75.3 h — outside the 48 h window G5a names, and never reached**.
- The only lists refreshing inside the window were `tif-mini` (24 h) and
  `phishdestroy` (12 h); the companion origin log shows both origins
  rotating validators daily, so no 304 was available to them.

**G5a was therefore untestable in the span observed.** It is re-declared
for the `0.3.1` soak, where `/data` carries `.validators` from `0.3.0` and
the first refresh of every list is conditional from `T0`.

### State at termination

All figures from the terminal pull
[`resoak-0.3.0/pull-final-20260901T0713Z-*`](resoak-0.3.0/), taken
immediately before the container stop: `uptime_seconds` 215 688, 600
samples, `stride 1`, full series from `T0`.

| Window | n | floor MiB | ceil MiB | half-to-half drift | G1 |
| --- | --- | --- | --- | --- | --- |
| W1 | 240 | 43.3 | 62.5 | −0.51 | pass |
| W2 | 240 | 52.0 | 94.8 | +0.99 | pass |
| W3 | 120 (partial) | 64.2 | 74.5 | — | no figure |

- **G2 floor series 43.3 → 52.0 → 64.2 MiB, still rising at termination.**
  W1's floor is the boot sample, so it measures cold start, not a plateau.
- Six-hour minima separate warm-up from the ratchet:

  | h | 0–6 | 6–12 | 12–18 | 18–24 | 24–30 | 30–36 | 36–42 | 42–48 | 48–54 | 54–60 |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | min MiB | 43.3 | 52.9 | 51.8 | 50.1 | 52.0 | 61.1 | 61.1 | 63.3 | 64.2 | 66.1 |

  The +9.1 MiB step into h30–36 lands on the h27.3 batch compile (16
  bodies). **After it, the minima still climb at +5.21 MiB/day** (least
  squares, h ≥ 30, n = 5) — a rate that is neither warm-up nor observer
  excursion, since those decay inside ~2 h.
- Growth is anonymous and unaccounted: `residual_bytes` minima 24.6 → 40.0
  MiB across h24–60, while `accounted_bytes` held ~29 MB, `rss_file_bytes`
  pinned at 9.7 MiB from h6, and `cache.bytes` oscillated 0.3–4.4 MiB.
- **`d420f38` has no evident mechanism against this.** It removes the
  history-parse churn, which is the component that already decays. The
  floor climb continues between pulls. Recorded here as a prediction against
  the `0.3.1` soak, not as a claim about it.
- **G3: 5 `peak_rss` steps, all attributed** to `list_fetch` activity at
  h3.3, h3.4, h15.4, h27.3, h27.4. `peak_rss` flat at 148.0 MiB
  (155 140 096 B) for the final 32.5 h, terminal pull included.
- **G4 clean at every pull, terminal included**: `events_dropped` 0,
  `swr.dropped` 0, `swr.failed` 0. Final `list_fetch`: `bodies` 22,
  `not_modified` **0**, `bytes_fetched` 50 734 788 (30.0 % of `B`);
  `ruleset.rules` 756 420.
- `allocator_committed_bytes` 154.4 → 323.6 MiB, monotone, stepping at
  compile waves — 4.4× RSS at termination.
- G5b, G5c, G6: not evaluated; the soak did not reach day 7.

### What carries forward

- The `adaptive` acceptance and p5-10 Stage B move again — to the `0.3.1`
  soak, as they moved off L.3. **Neither L.3 nor this soak carries them.**
- `0.3.1` ships `d420f38` (slim history rows) plus the version bump.
- **The `0.3.1` pre-declaration replaces the "never pass `?fields=`" rule
  with an explicit 15-name set — every field except `upstreams`** —
  verified on device to drop only `upstreams`, to preserve `memory` (all 6
  subkeys), `list_fetch`, `allocator_committed_bytes`, `rss_anon_bytes`,
  `rss_file_bytes`, and to return a byte-identical `rss_bytes`/`peak_rss`
  series at `stride 1` (21-row window: 41 192 B full vs 18 869 B slim).
- The `+16` unavoidable-bodies allowance (§G5b) **does not carry** —
  `/data` retains `.validators`, so `0.3.1`'s first refresh per list is
  conditional.
- The companion `origin-log.tsv` runs on the dev box and is unaffected by
  the swap; it already predates `0.3.1`'s `T0` by four days, satisfying
  §Method's "starts before the container start" for the new soak.
