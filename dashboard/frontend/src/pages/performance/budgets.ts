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

/** `Blocked query`, `cache_hit stage` and `forward stage overhead added by
 *  engine` all take the same p99 budget (PERFORMANCE.md §Budgets). */
export const STAGE_BUDGET_MS = 1;

/** The chip beside each tile figure. */
export const STAGE_BUDGET_CHIP = 'BUDGET < 1 ms';

/** The dashed marker's own caption inside the plot. */
export const STAGE_BUDGET_LABEL = '1.0 ms — budget';

/** `DNS sustained throughput ≥ 10 000 QPS`, measured at 20 k+ on the RB5009. */
export const SUSTAINED_QPS = '20 k+';
export const SUSTAINED_QPS_NOTE = 'sustained capacity, measured on the RB5009';
