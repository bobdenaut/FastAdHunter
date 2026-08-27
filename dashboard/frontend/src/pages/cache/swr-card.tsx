import type { Telemetry } from '../../api/types';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { RefreshCluster } from '../../components/refresh-cluster';
import type { RefreshRegistry } from '../../refresh/registry';
import { CounterTable } from './counter-table';

/**
 * `counters.swr`, five fields verbatim.
 *
 * The `telemetry` refresh cluster sits here rather than in the page header:
 * one cluster per distinct polled endpoint, and this card and the cleanup card
 * beneath it are the only things on the page reading `/telemetry`. The header's
 * cluster owns `/cache`.
 */
export function SwrCard({
  telemetry,
  registry,
}: {
  telemetry: Telemetry | null;
  registry: RefreshRegistry;
}) {
  const swr = telemetry?.counters.swr ?? null;
  return (
    <Card
      title="Stale-while-revalidate"
      secondary="refresh ahead of expiry"
      tools={<RefreshCluster registry={registry} endpoint="telemetry" />}
    >
      {swr === null ? (
        <EmptyState title="Not read yet" />
      ) : (
        <>
          <CounterTable
            rows={[
              { label: 'enqueued', value: swr.enqueued.toLocaleString() },
              { label: 'deduplicated', value: swr.deduplicated.toLocaleString() },
              { label: 'completed', value: swr.completed.toLocaleString(), tone: 'good' },
              { label: 'failed', value: swr.failed.toLocaleString() },
              { label: 'dropped', value: swr.dropped.toLocaleString() },
            ]}
          />
          <p class="note">
            Deduplicated means the same name was already queued — that work was
            saved, not lost. Dropped is the one to watch: it means the queue was
            full.
          </p>
        </>
      )}
    </Card>
  );
}
