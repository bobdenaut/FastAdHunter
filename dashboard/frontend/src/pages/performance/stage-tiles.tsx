import type { PerfItem, PerfLatency } from '../../api/types';
import { latencyMsLabel } from '../../charts/format';
import { budgetProximity, latencyMs, latestLatency } from '../../derive';
import { STAGE_BUDGET_CHIP, STAGE_BUDGET_MS } from './budgets';

interface Stage {
  key: keyof PerfLatency;
  label: string;
}

/**
 * The three stages partition every resolved query, and a latency figure without
 * its stage is meaningless — a fast average is usually just a high cache-hit
 * rate. `forward` is the **engine's** share only: upstream round-trip time is
 * excluded, which is what PERFORMANCE.md's budget is written against.
 */
const STAGES: readonly Stage[] = [
  { key: 'block_p99', label: 'block stage, p99' },
  { key: 'cache_hit_p99', label: 'cache-hit stage, p99' },
  { key: 'forward_p99', label: 'forward stage, engine overhead p99' },
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
  seconds,
  unread,
}: {
  label: string;
  seconds: number | null;
  /** The range's read failed, so there is no figure for it — distinct from a
   *  range that was read and held nothing. */
  unread: boolean;
}) {
  // KTD7 — an exact `0.0` is a stage with no queries in that interval, not a
  // measurement of zero. A real reading is a bucket upper bound.
  const ms = seconds === null ? null : latencyMs(seconds);
  const bar = ms === null ? null : budgetProximity(ms, STAGE_BUDGET_MS);

  return (
    <section class="card stage-tile">
      <div class="bd">
        <div class="note">{label}</div>
        <div class="stage-figure">
          <span class="big mono">{ms === null ? '—' : latencyMsLabel(ms)}</span>
          {ms !== null && <span class="note">ms</span>}
          {/* Neutral, not `good`: the chip is the budget's own label and states
              nothing about this reading. The bar beneath it is the one thing on
              the tile that carries a verdict, and it flips at the budget. */}
          <span class="pill neutral stage-budget">{STAGE_BUDGET_CHIP}</span>
        </div>
        <div class="bar stage-bar-track">
          {bar !== null && (
            <span
              class={bar.over ? 'stage-bar-fill over' : 'stage-bar-fill'}
              style={{ width: `${String(bar.percent)}%` }}
            />
          )}
        </div>
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
