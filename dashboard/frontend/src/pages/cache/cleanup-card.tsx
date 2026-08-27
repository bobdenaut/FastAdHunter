import type { Telemetry } from '../../api/types';
import { formatMiB, microsLabel } from '../../charts/format';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { CounterTable } from './counter-table';

/**
 * `counters.cache_cleanup`, four fields.
 *
 * **`last_duration_micros` is a last-value gauge** — the only one in a block of
 * cumulative counters (API.md §telemetry). It describes the most recent sweep,
 * so it is rendered as a current value and never as a series: deltaing it, or
 * plotting it over time, produces nonsense. E9 is the unit conversion and
 * nothing else.
 */
export function CleanupCard({ telemetry }: { telemetry: Telemetry | null }) {
  const cleanup = telemetry?.counters.cache_cleanup ?? null;
  return (
    <Card title="Background cleanup">
      {cleanup === null ? (
        <EmptyState title="Not read yet" />
      ) : (
        <CounterTable
          rows={[
            { label: 'runs', value: cleanup.runs.toLocaleString() },
            {
              label: 'entries removed',
              value: cleanup.entries_removed.toLocaleString(),
            },
            { label: 'bytes freed', value: formatMiB(cleanup.bytes_freed) },
            {
              label: 'last run took',
              value: `${microsLabel(cleanup.last_duration_micros)} ms`,
            },
          ]}
        />
      )}
    </Card>
  );
}
