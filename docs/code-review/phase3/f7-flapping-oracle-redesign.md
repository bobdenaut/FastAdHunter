# F7 — the flapping oracle, redesigned

Finding: [main-phase3-integration-audit.md](main-phase3-integration-audit.md) F7.
Code: `crates/fah-dns/tests/adaptive_behaviour.rs`, `b5_recovery_and_flapping`.
Semantics read at `1aa4362` in `crates/fah-dns/src/upstream/health.rs`.

## Summary

The p99 guard was not repairable. Measuring the existing script proved it never
produced the behaviour it claimed to check: on every arm the penalty window at
the start of the flapping phase was longer than the whole flapping phase, so the
endpoint was never eligible for re-selection there. The one eligibility observed
across three arms was a penalty started in `black_hole` that expired 2.9 s past a
phase boundary — a timing coincidence inside the ±25 % jitter.

The oracle is replaced by a count of penalty applications during flapping,
bounded by the backoff schedule. The script gains a reset phase and absolute
flapping phases so the property is exercised by construction rather than by
chance.

## Decisions

- **The property is the damping, not a latency tail.** Under flapping, correct
  behaviour escalates the penalty round, so penalties become rarer. The
  regression — the round resetting on each short recovery — keeps the window at
  the base, so they do not.
- **`p99` stays printed, never asserted.** No arm collects enough flapping
  samples for a p99 to be an estimate rather than a single order statistic.
- **The allowance is computed, not chosen.** It follows from `nominal_penalty()`
  and the jitter's lower bound.
- **Arm 2.5 cannot discriminate this regression.** Recorded, not worked around.
- No production code changed.

## Semantics, as read

| # | Behaviour | Where |
| --- | --- | --- |
| 1 | The round resets to 1 **only at the moment of penalizing**, and only if the endpoint is `Healthy` and has been continuously healthy for at least `penalty_max_ms`. Otherwise `round + 1`, capped at `ROUND_MAX = 15` | `health.rs:190` `next_round` |
| 2 | The deadline is `now + nominal(round) x jitter`, jitter `75..=125 %` from the low byte of the clock | `health.rs:171` `penalty` |
| 3 | A `Penalized` endpoint is skipped until its deadline passes; then it is CAS'd to `Probing` and returned as `Selected::Probe`, at most one probe per query | `health.rs:333` `select` |
| 4 | A successful probe sets `Healthy`, clears `consecutive_failures`, **preserves `penalty_round`**, and stamps healthy-since | `health.rs:254` |
| 5 | Once `Healthy` the endpoint is selected normally; a timeout is a `HardFailure`, so it takes `penalty_failures` of them to penalize again, and that penalty escalates the round | `health.rs:116`, `:261` |

## Why the round cannot reset during flapping

The longest healthy run inside the flapping phase is one `flap_up`, 3 000 ms.
Rule 1 needs `penalty_max_ms` of continuous health. The smallest arm's
`penalty_max_ms` is 3 750 ms. So `3000 < 3750` holds on every arm and the round
only ever increases there. The test asserts this premise rather than assuming it.

## Why `healthy_reset = 2.0` units

The endpoint becomes `Healthy` when the penalty left over from `black_hole`
expires, not when the healthy phase starts. Worst case that penalty is applied at
the very end of `black_hole` with the capped window jittered to 125 %, so
recovery is up to **1.25 units** after `black_hole` ends.

| | units |
| --- | --- |
| healthy time after `black_hole` (`healthy_mid` + `healthy_reset`) | 0.6 + 2.0 = 2.6 |
| worst-case delay before recovery | 1.25 |
| healthy stretch at flap start | **1.35** |
| reset threshold (`penalty_max_ms`) | 1.0 |

Margin 0.35 units. `healthy_reset = 1.1` was rejected: it gives
`0.6 + 1.1 - 1.25 = 0.45` units, below the threshold. The strict minimum is 1.65.

**The reset is observed at the penalty, not at the recovery.** Semantics 4 says a
successful probe preserves `penalty_round`, so the state word still carries the
round earned in `black_hole` when flapping begins — measured as 3, 5 and 7 on the
three arms. `next_round` evaluates the reset when the *next* penalty is applied.
The check is therefore that the first penalty inside the flapping phase uses
round 1, which arm 2.5 confirms: `penalty_rounds` opens with
`("flap_down_1", 1)`. An earlier draft of this file asserted
`round_at_flap_start == 1` and contradicted its own semantics table.

## The allowance

`allowed = 1 + max k` such that the first `k` nominal windows, each at its
jitter minimum of 75 %, fit inside the flapping phase. The `+1` is the window
that overhangs the end.

`T = FLAP_CYCLES x (flap_down + flap_up) = 6 x 6 000 = 36 000 ms`.

| Arm | nominal windows (ms, capped) | cumulative at 75 % | k | **allowed** |
| --- | --- | --- | --- | --- |
| 2.5 | 1500, 3000, 3750, 3750, … | 1125, 3375, 6187, 9000, 11812, 14625, 17437, 20250, 23062, 25875, 28687, 31500, 34312 · *37124 > 36000* | 13 | **14** |
| 12.5 | 1500, 3000, 6000, 12000, 18750, … | 1125, 3375, 7875, 16875, 30937 · *45000 > 36000* | 5 | **6** |
| 37.5 | 1500, 3000, 6000, 12000, 24000, 48000, 56250 | 1125, 3375, 7875, 16875, 34875 · *70875 > 36000* | 5 | **6** |

Each arm's own `penalty_max_ms` enters through `nominal_penalty()`, which is why
the three allowances differ instead of sharing one number.

## Timeline — correct behaviour, jitter 1.0

Round 1 at flap start. `down_i = [(i-1)*6000, +3000)`, `up_i = [+3000, +6000)`.

Arms 12.5 and 37.5:

| # | t (ms) | triggered by | round to window | deadline | lands in |
| --- | --- | --- | --- | --- | --- |
| 1 | 100 | re-selection, 2 failures | 1 to 1 500 | 1 600 | `down_1`, probe fails |
| 2 | 1 600 | failed probe | 2 to 3 000 | 4 600 | `up_1`, probe succeeds |
| 3 | 6 100 | re-selection | 3 to 6 000 | 12 100 | `down_3`, fails |
| 4 | 12 100 | failed probe | 4 to 12 000 | 24 100 | `down_5`, fails |
| 5 | 24 100 | failed probe | 5 to 18 750 / 24 000 | 42 850 / 48 100 | past 36 000 |

**5 penalties**, allowance 6.

Arm 2.5, where the window caps at 3 750 — 1.25 x one phase:

| # | t (ms) | round to window | deadline | lands in |
| --- | --- | --- | --- | --- |
| 1 | 100 | 1 to 1 500 | 1 600 | `down_1`, fails |
| 2 | 1 600 | 2 to 3 000 | 4 600 | `up_1`, succeeds |
| 3 | 6 100 | 3 to 3 750 | 9 850 | `up_2`, succeeds |
| 4 | 12 100 | 4 to 3 750 | 15 850 | `up_3`, succeeds |
| 5 | 18 100 | 5 to 3 750 | 21 850 | `up_4`, succeeds |
| 6 | 24 100 | 6 to 3 750 | 27 850 | `up_5`, succeeds |
| 7 | 30 100 | 7 to 3 750 | 33 850 | `up_6`, succeeds |

**7 penalties**, allowance 14.

## The regression, and what it costs

Regression: `penalty_round` resets on every short recovery. The window returns to
`penalty_base_ms` after each successful probe and escalates only to round 2
inside the following down-phase, so the pattern repeats every cycle: **two
penalties per cycle, 6 cycles, about 12 penalties on every arm.**

| Arm | correct | allowed | regression | separated? |
| --- | --- | --- | --- | --- |
| 2.5 | 7 | 14 | ~12 | **no** |
| 12.5 | 5 | 6 | ~12 | yes, 2x over |
| 37.5 | 5 | 6 | ~12 | yes, 2x over |

## Why arm 2.5 cannot discriminate it

The regression is stuck at the round-2 window, `2 x penalty_base_ms` = 3 000 ms.
Arm 2.5's ceiling is `penalty_max_ms` = 2.5 x 1 500 = 3 750 ms. The two differ by
25 %, so no count of events separates them. This is a property of the ratio, not
of the phase lengths or of the oracle: any arm with a ratio below about 4 is
blind to it.

Arm 2.5 still asserts `penalties <= 14`, so it does not pass silently and it
still catches coarser regressions — a penalty that never applies, a probe storm.
For **this** regression the discriminating arms are 12.5 and 37.5.

## Validity — no result may be undecidable

Every one of these fails the arm rather than printing a note:

| Condition | Why it invalidates the count |
| --- | --- |
| `flap_up >= penalty_max_ms` | the round could reset inside flapping and the allowance would not hold |
| the first penalty inside the flapping phase does not use round 1 | `healthy_reset` did not carry a long enough healthy stretch, so the windows are not `nominal(1..k)` |
| any `flap_down_i` carried no query | that cycle never gave the pool a chance to re-select |
| failures below failed probes | the counters disagree |

## Cost

Flapping is 36 s on every arm; `healthy_reset` adds 2.0 units. Whole test about
**10.5 minutes**, so it stays `#[ignore]` and out of the gate.

## Validation — 2026-09-14, Windows dev box, idle

Both runs: `cargo test --all-features -p fah-dns --test adaptive_behaviour
b5_recovery_and_flapping -- --include-ignored --test-threads=1`, 627.9 s each.

**Normal run: `ok. 1 passed; 0 failed`.**

**Mutation run.** `next_round` temporarily reduced to `if w.state ==
State::Healthy { 1 }`, dropping the continuous-health requirement so any
recovery resets the round — the regression this oracle exists for. Reverted
immediately after; `git status` and `git diff --stat` on `crates/fah-dns/src/`
are both empty and `next_round` is byte-identical to `1aa4362`.

| Arm | predicted | normal | mutated | allowance | oracle under mutation |
| --- | --- | --- | --- | --- | --- |
| 2.5 | 7 / ~12 | **7** | **12** | 14 | passes — non-discriminating, as documented |
| 12.5 | 5 / ~12 | **5** | **12** | 6 | **FAIL** |
| 37.5 | 5 / ~12 | **5** | **12** | 6 | **FAIL** |

Every figure matches the model, both normal and mutated. The mutated arms report
`12 penalties over 36000 ms of flapping against an allowance of 6`.

## Findings

1. **Within an arm, the first failing assertion hid every later one.** Arm
   isolation was added between the three arms, not inside them, so the first
   mutation run failed on `penalty_round must climb` and on `dead_high_water` and
   the count oracle never executed. Fixed by collecting findings instead of
   panicking: `assert!` became a local `check!` that pushes onto a list, the arm
   asserts once at the end. No invariant was removed and nothing was reordered.
   The second mutation run then reported 5, 7 and 7 failed checks per arm with
   the count oracle among them. Without this the oracle could have shipped
   unexercised.
2. **Two pre-existing bounds were computed from `round_at_flap_start`**, the
   round carried in the state word at flap start, which `healthy_reset` now makes
   stale by design. `flap_windows` derived from it gave 2 and 1 against 4 observed
   probes, and the monotonicity loop forbade the legitimate reset. Both now use
   the same geometric accounting as the allowance; the first flapping penalty is
   checked more strictly than before, since it must equal round 1 rather than
   merely climb.
3. **An earlier draft of this file asserted `round_at_flap_start == 1`**, which
   contradicts its own semantics row 4. Corrected above.

## Workspace gate — not green, and not because of this change

`cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --
-D warnings` are clean. `cargo test --all-features --workspace --no-fail-fast`
was run twice on this tree and failed once each time, on a **different**
`fah-http` test each run. Both pass when run alone, three times each. This diff
touches only `crates/fah-dns/tests/adaptive_behaviour.rs` and cannot reach
`fah-http`.

**Run 1 — `a_saturated_https_lane_leaves_the_http_lane_bounded_and_leaks_no_permit`**
(`crates/fah-http/tests/sni.rs:838`):

```text
assertion `left == right` failed: every accepted HTTPS socket is judged once the ceiling frees up
  left: 2
 right: 42
```

**Run 2 — `warm_intercepted_requests_allocate_a_steady_amount`**
(`crates/fah-http/tests/intercept_alloc.rs:328`):

```text
intercept/allocations over 64 warm requests (intercepted pass-through GET): [(3072, 1309120), (3173, 1728912), (3200, 1835968), (3200, 1835968)]
intercept/allocations over 64 warm requests (intercepted blocked script): [(1600, 1209664), (1600, 1209664), (1600, 1209664), (1601, 1209693)]
64 warm requests (intercepted blocked script) allocated 1601; the ceiling is 25 per request
```

The second one is diagnosable rather than merely flaky: `ceiling_per_request: 25`
(`intercept_alloc.rs:279`) times 64 requests is 1600, and the measured cost is
**exactly** 1600. The pass-through arm sits exactly on its 3200 likewise. Both
ceilings are calibrated to the measured value with no headroom, so one extra
allocation from fragmentation under load fails them. That is the same family as
[allocation-oracles-that-only-hold-on-an-idle-machine.md](../../solutions/design-patterns/allocation-oracles-that-only-hold-on-an-idle-machine.md),
whose fix deliberately left the per-operation ceilings alone.

Both names are recorded here so they are not lost, per §2 of that file. Neither
is touched by this task: raising a ceiling needs a derived margin, not a larger
number, and that is its own go.

**F7 is validated. The workspace gate is blocked by two pre-existing `fah-http`
tests, independent of this diff.**

PASS WITH DEFERRED FINDINGS

## Remaining TODOs

- Arm 2.5 remains blind to the round-reset regression through the count oracle.
  Under mutation it fails on other checks, so the suite catches the regression,
  but the count is 12 against an allowance of 14. Structural, recorded, not
  worked around.
- The two `fah-http` allocation and saturation ceilings above want their own
  task: derive a margin from the signal a real regression would produce, rather
  than raising the number until the test stops failing.
