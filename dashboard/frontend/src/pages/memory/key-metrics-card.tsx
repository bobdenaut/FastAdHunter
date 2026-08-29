import type { DebugMemory, Telemetry } from '../../api/types';

import { Card } from '../../components/card';
import { formatUptime } from '../../time';
import {
  CEILING_BUDGET,
  budgetLabel,
  CEILING_LABEL,
  STEADY_STATE_BUDGET,
  STEADY_STATE_LABEL,
} from './budgets';

/**
 * The figures that answer "is this fine" without the reader knowing
 * PERFORMANCE.md.
 *
 * **Headroom is the one number here that is actually actionable**, and it is
 * the reason the budget is worth carrying on the page at all: `54.9 MiB` on its
 * own says nothing, `70.4 MB of headroom` says whether there is room on a box
 * that shares 1 GB with RouterOS.
 *
 * Both budgets come from PERFORMANCE.md, not from any response — nothing in the
 * API reports a budget, and a figure derived from a documented constant is
 * labelled as one rather than presented as a measurement.
 */
export function KeyMetricsCard({
  memory,
  telemetry,
}: {
  memory: DebugMemory | null;
  /**
   * The compiled rule count and uptime — the two rows `/debug/memory` does not
   * carry. `null` while the one-shot read is in flight or after it failed, and
   * both rows render an em dash rather than a zero.
   */
  telemetry: Telemetry | null;
}) {
  const rss = memory?.process_rss ?? null;
  const headroom = rss === null ? null : STEADY_STATE_BUDGET - rss;

  return (
    <Card title="Key metrics" secondary="this instant">
      <table class="kv-table">
        <tbody>
          <Row
            label="steady-state budget"
            value={budgetLabel(STEADY_STATE_BUDGET)}
          />
          <Row
            label="headroom"
            value={
              headroom === null
                ? '—'
                : `${(headroom / 1_000_000).toFixed(1)} MB · ${((headroom / STEADY_STATE_BUDGET) * 100).toFixed(0)} %`
            }
            tone={headroom !== null && headroom > 0 ? 'good' : undefined}
          />
          <Row
            label="hard-ceiling budget"
            value={budgetLabel(CEILING_BUDGET)}
          />
          <Row
            label="ruleset rules"
            value={
              telemetry === null
                ? '—'
                : telemetry.ruleset.rules.toLocaleString()
            }
          />
          {/* The artboard has a `purge delay` row between these two. It is not
              drawn: the value is `MIMALLOC_PURGE_DELAY`, an allocator env var,
              and no endpoint reports it. Phase constraint 1 — no figure the API
              does not support, and a hard-coded "10 s" would be a reading
              nobody took. */}
          <Row
            label="cache entries"
            value={
              memory === null ? '—' : memory.cache_entries.toLocaleString()
            }
          />
          <Row
            label="uptime"
            value={
              telemetry === null
                ? '—'
                : formatUptime(telemetry.process.uptime_seconds)
            }
          />
        </tbody>
      </table>
      <p class="note kv-note">
        <b>Budgets, not limits</b> — nothing enforces them at runtime (the
        container runs <span class="mono">memory-high=unlimited</span>), so
        crossing one is something to investigate against a pre-change build,
        never a kill. {STEADY_STATE_LABEL} and {CEILING_LABEL} are drawn as
        markers for exactly that reason.
      </p>
    </Card>
  );
}

function Row({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone?: 'good' | undefined;
}) {
  return (
    <tr>
      <td>{label}</td>
      <td class={tone === 'good' ? 'num kv-good' : 'num'}>{value}</td>
    </tr>
  );
}
