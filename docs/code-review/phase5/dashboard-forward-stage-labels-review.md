# Review — ad-hoc: forward-stage labels on the Performance page

Reviewed working tree (uncommitted, base `2384109`). Frontend only, 6 files:
`performance.tsx`, `performance/budgets.ts`, `performance/latency-chart.tsx`,
`performance/stage-tiles.tsx`, `performance.test.tsx`, `components.css`.
A per-endpoint upstream-RTT feature built in the same session was reverted and
re-planned as [p5-11-upstream-rtt.md](../../../plan/wip/phase5/p5-11-upstream-rtt.md);
it is not part of this change.

## Summary

- The page claimed `forward` "excludes upstream round-trip time: these three
  stages measure only what FastAdHunter adds", and drew the `< 1 ms` budget
  chip and proximity bar beside a forward p99 of ~50 ms — a permanent 50×
  breach that never happened. Both claims were wrong:
  `fah-metrics/src/registry.rs:38` documents `duration_forward` as end-to-end
  including the upstream await and, for the RFC 8767 fallback, the failed
  attempt before it. PERFORMANCE.md §Budgets writes its `< 1 ms` row against
  *engine overhead*, which is not the served figure.
- Fix is copy-only, no Rust and no metrics change: forward tile relabeled
  "upstream round trip included", budget chip/bar removed from it (replaced
  by a `NO BUDGET — END TO END` chip and a note), chart title/legend/footnote
  corrected. `block` and `cache_hit` keep the budget — they never wait on the
  network, so the row applies.
- Every changed string traces to the code it describes; nothing is derived
  that the API does not serve (phase invariant, frontend review focus §1).
- Gates: frontend 941/941 green, `tsc --noEmit` clean, crates untouched
  (identical to HEAD; workspace clippy/tests were green at `2384109`).

## Findings

1. **LOW — FIXED — PERFORMANCE.md §Budgets row had no measured counterpart.**
   The `forward stage overhead added by engine, p99 < 1 ms` row referred to a
   figure nothing measures — isolating the engine's share needs a timer
   around the upstream await, which does not exist. **Fix (owner-approved):**
   the row's Measured cell now says "not measured — see note under the
   table", and the paragraph below the table states that the served `forward`
   histogram is end-to-end (upstream RTT and the RFC 8767 failed attempt
   included) and that the dashboard's forward tile carries no budget for that
   reason. Pre-existing condition surfaced by this change, not introduced by
   it.
2. **LOW — FIXED — a saturated percentile read as an exact figure.** The
   stage quantile saturates at the histograms' last finite bucket, 0.1 s
   (`fah-metrics/src/histogram.rs` `BUCKETS_SECONDS`): a served percentile
   equal to 100 ms is a floor, not a reading, and the tile printed it bare.
   (An earlier draft of this finding misstated the bound as 50 ms — the
   observed live forward p99 of 50.000 ms sits in the 0.05 s bucket and is
   *not* saturated; only 100.000 ms is.) **Fix:** `STAGE_TOP_BUCKET_MS = 100`
   in `budgets.ts`, documented as a mirror of the Rust constant; a tile
   figure at or above it renders as `≥ 100.000`; the chart footnote names the
   bound ("saturate at the top finite bucket, 100 ms — a value sitting there
   is a floor"). Covered by a new test rendering `forward_p99: 0.1` and
   asserting `≥ 100.000` appears while unsaturated figures stay bare.

## Decisions

- Forward tile carries **no budget** rather than a borrowed one; the missing
  engine-overhead timer is real pipeline work and is explicitly out of scope
  of p5-11 too — it needs its own decision.
- The empty bar space on the forward tile is filled by the explanatory note so
  the three tiles keep lining up (`stage-nobudget-note` in components.css).

## Files changed

- `dashboard/frontend/src/pages/performance.tsx` — subtitle: "Latency by
  stage" instead of "What the engine adds".
- `dashboard/frontend/src/pages/performance/budgets.ts` — budget scoped to
  `block`/`cache_hit`; `STAGE_NO_BUDGET_CHIP` added; rationale comment.
- `dashboard/frontend/src/pages/performance/stage-tiles.tsx` — per-stage
  `budget` flag; forward tile: new label, no chip/bar, explanatory note.
- `dashboard/frontend/src/pages/performance/latency-chart.tsx` — title,
  legend ("incl. upstream RTT"), corrected footnote.
- `dashboard/frontend/src/pages/performance.test.tsx` — assertions updated;
  new test: forward tile has no budget chip and no proximity bar.
- `dashboard/frontend/src/styles/components.css` — `stage-nobudget` /
  `stage-nobudget-note` rules.

## Fixes applied (2026-08-31, owner-approved)

- PERFORMANCE.md §Budgets: forward row marked not measured; note under the
  table states the end-to-end timing and the tile's missing budget.
- `budgets.ts`: `STAGE_TOP_BUCKET_MS` added; `stage-tiles.tsx`: saturated
  figure renders `≥`; `latency-chart.tsx`: footnote names the 100 ms bound;
  `performance.test.tsx`: saturation test added.
- Verification: `tsc --noEmit` clean; frontend 942/942 green; crates
  untouched.

## Remaining TODOs

None.

**Verdict: PASS**
