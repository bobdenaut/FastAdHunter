/**
 * **Documentation-sourced constants, not figures this page measured.**
 *
 * Every value here is read out of PERFORMANCE.md §Budgets and is drawn as a
 * marker — a dashed line, a chip, a caption — never as something the engine
 * enforces. Nothing enforces a budget at runtime: the container runs
 * `memory-high=unlimited`, so a crossing is a budget breach to investigate
 * against a pre-change build, not a failure the engine will act on.
 *
 * The measured throughput figure carries its device, because it is the only
 * honest way to state it: it is an RB5009 reading, not something this browser
 * or this container established.
 */

/** `Blocked query` and `cache_hit stage` take this p99 budget (PERFORMANCE.md
 *  §Budgets). **`forward` does not.** PERFORMANCE.md's third `< 1 ms` row is
 *  written against *overhead added by the engine*, and the served
 *  `forward_p99` is not that figure: `duration_forward` is timed end-to-end
 *  including the upstream round trip and, for the RFC 8767 fallback, the
 *  failed attempt before it. Isolating the engine's share needs a
 *  pipeline-side timer around the upstream await, which does not exist — so
 *  the forward tile carries no budget rather than a borrowed one it breaches
 *  by construction. */
export const STAGE_BUDGET_MS = 1;

/** The chip beside a tile whose stage the budget row covers. */
export const STAGE_BUDGET_CHIP = 'BUDGET < 1 ms';

/** The chip beside a tile whose stage it does not — `forward`. Says why the
 *  figure has nothing to be compared against rather than leaving the space
 *  blank, which reads as an omission. */
export const STAGE_NO_BUDGET_CHIP = 'NO BUDGET — END TO END';

/** The dashed marker's own caption inside the plot. */
export const STAGE_BUDGET_LABEL = '1.0 ms — budget';

/** The one constant here sourced from code rather than PERFORMANCE.md: the
 *  last finite bound of the stage histograms' buckets
 *  (`fah-metrics/src/histogram.rs`, `BUCKETS_SECONDS`), in milliseconds. The
 *  quantile saturates there — a served percentile equal to it is a floor, not
 *  a reading, so the tile prints it with a `≥`. */
export const STAGE_TOP_BUCKET_MS = 100;

/** `DNS sustained throughput ≥ 10 000 QPS`, measured at 20 k+ on the RB5009. */
export const SUSTAINED_QPS = '20 k+';
export const SUSTAINED_QPS_NOTE = 'sustained capacity, measured on the RB5009';
