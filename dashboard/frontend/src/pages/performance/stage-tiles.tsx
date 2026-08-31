import type { PerfItem, PerfLatency } from '../../api/types';
import { latencyMsLabel } from '../../charts/format';
import { budgetProximity, latencyMs, latestLatency } from '../../derive';
import {
  STAGE_BUDGET_CHIP,
  STAGE_BUDGET_MS,
  STAGE_NO_BUDGET_CHIP,
  STAGE_TOP_BUCKET_MS,
} from './budgets';

interface Stage {
  key: keyof PerfLatency;
  label: string;
  /** Whether PERFORMANCE.md's `< 1 ms` p99 row applies to this stage — see
   *  [`STAGE_BUDGET_MS`]. `forward` is timed end-to-end, so it does not. */
  budget: boolean;
}

/**
 * The three stages partition every resolved query, and a latency figure without
 * its stage is meaningless — a fast average is usually just a high cache-hit
 * rate. `block` and `cache_hit` never wait on the network and carry the budget;
 * `forward` is the whole round trip, upstream time included, and carries none.
 */
const STAGES: readonly Stage[] = [
  { key: 'block_p99', label: 'block stage, p99', budget: true },
  { key: 'cache_hit_p99', label: 'cache-hit stage, p99', budget: true },
  {
    key: 'forward_p99',
    label: 'forward stage, p99 — upstream round trip included',
    budget: false,
  },
];

export function StageTiles({
  items,
  error,
}: {
  items: readonly PerfItem[];
  error: Error | null;
}) {
  // A failed read is not an empty range. The previous range's rows stay in
  // state so the plot survives a retry, but a tile attributes its figure to the
  // *selected* range — printing the old window's number here would name the
  // wrong one, and the read that would have replaced it never landed.
  const latest = error === null ? latestLatency(items) : null;
  return (
    <>
      <div class="row c3 stage-tiles">
        {STAGES.map((stage) => (
          <StageTile
            key={stage.key}
            label={stage.label}
            budget={stage.budget}
            seconds={latest === null ? null : latest[stage.key]}
            unread={error !== null}
          />
        ))}
      </div>
      <p class="note stage-tiles-note">
        Each figure is the latest <b>served</b> sample of the selected range.
        Decimation drops whole rows, so at the wider ranges that is not the same
        thing as now.
      </p>
    </>
  );
}

function StageTile({
  label,
  budget,
  seconds,
  unread,
}: {
  label: string;
  /** A stage the `< 1 ms` row is written against. A stage without one shows no
   *  chip and no bar: a borrowed budget would read as a permanent breach. */
  budget: boolean;
  seconds: number | null;
  /** The range's read failed, so there is no figure for it — distinct from a
   *  range that was read and held nothing. */
  unread: boolean;
}) {
  // KTD7 — an exact `0.0` is a stage with no queries in that interval, not a
  // measurement of zero. A real reading is a bucket upper bound.
  const ms = seconds === null ? null : latencyMs(seconds);
  const bar = ms === null || !budget ? null : budgetProximity(ms, STAGE_BUDGET_MS);

  return (
    <section class="card stage-tile">
      <div class="bd">
        <div class="note">{label}</div>
        <div class="stage-figure">
          {/* The quantile saturates at the histogram's last finite bucket: a
              figure equal to it is a floor, and printing it bare would claim
              an exact p99 the buckets cannot resolve. */}
          <span class="big mono">
            {ms === null
              ? '—'
              : ms >= STAGE_TOP_BUCKET_MS
                ? `≥ ${latencyMsLabel(ms)}`
                : latencyMsLabel(ms)}
          </span>
          {ms !== null && <span class="note">ms</span>}
          {/* Neutral, not `good`: the chip is the budget's own label and states
              nothing about this reading. The bar beneath it is the one thing on
              the tile that carries a verdict, and it flips at the budget. */}
          {budget ? (
            <span class="pill neutral stage-budget">{STAGE_BUDGET_CHIP}</span>
          ) : (
            <span class="pill neutral stage-nobudget">{STAGE_NO_BUDGET_CHIP}</span>
          )}
        </div>
        {budget && (
          <div class="bar stage-bar-track">
            {bar !== null && (
              <span
                class={bar.over ? 'stage-bar-fill over' : 'stage-bar-fill'}
                style={{ width: `${String(bar.percent)}%` }}
              />
            )}
          </div>
        )}
        {!budget && (
          <p class="note stage-nobudget-note">
            Network time, not engine time. The <b>&lt; 1 ms</b> row is written
            against the engine's own share of a forward, which is not measured
            separately — so there is nothing here to compare against.
          </p>
        )}
        {ms === null && (
          <p class="note stage-idle">
            {unread
              ? 'The read for this range failed — no figure rather than one from another range.'
              : seconds === null
                ? 'No sample in the selected range.'
                : 'No traffic in this stage in the latest sample.'}
          </p>
        )}
      </div>
    </section>
  );
}
