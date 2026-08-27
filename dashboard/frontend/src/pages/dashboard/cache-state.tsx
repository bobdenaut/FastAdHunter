import type { CacheUsage } from '../../api/types';
import { percent1 } from '../../charts/format';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { RefreshCluster } from '../../components/refresh-cluster';
import { StageBar } from '../../components/stage-bar';
import type { RefreshRegistry } from '../../refresh/registry';
import { Link } from '../../router/link';

/**
 * Every figure is a `/cache` field except `free`, which is the stated
 * derivation `capacity − entries` (R7) and is labelled `free` rather than
 * dressed up as a measured band.
 */
export function CacheState({
  cache,
  registry,
  className,
}: {
  cache: CacheUsage | null;
  registry: RefreshRegistry;
  className?: string;
}) {
  return (
    <Card
      title="Cache state"
      secondary={
        cache === null
          ? undefined
          : `entries ${cache.entries.toLocaleString()} / ${cache.capacity.toLocaleString()}`
      }
      tools={<RefreshCluster registry={registry} endpoint="cache" />}
      className={className}
    >
      {cache === null ? (
        <EmptyState title="Not read yet" />
      ) : (
        <>
          <StageBar
            segments={[
              { label: 'fresh', value: cache.fresh, colour: 'var(--cache-fresh)' },
              { label: 'stale', value: cache.stale, colour: 'var(--cache-stale)' },
              {
                label: 'expired',
                value: cache.expired,
                colour: 'var(--cache-expired)',
              },
              {
                label: 'free',
                value: Math.max(0, cache.capacity - cache.entries),
                colour: 'var(--cache-free)',
              },
            ]}
          />
          <div class="figure-grid">
            <div>
              <span class="note">entry load</span>
              <span class="mono">{percent1(cache.load_percent)}%</span>
            </div>
            <div>
              <span class="note">byte load</span>
              <span class="mono">{percent1(cache.byte_load_percent)}%</span>
            </div>
            <div>
              <span class="note">hits</span>
              <span class="mono">{cache.hits.toLocaleString()}</span>
            </div>
            <div>
              <span class="note">evictions</span>
              <span class="mono">{cache.evictions.toLocaleString()}</span>
            </div>
          </div>
          <p class="note">
            Entry load is the bound closest to evicting. Stale answers only
            after a failed forward — it is serve-stale insurance, not dead
            weight.
          </p>
        </>
      )}
      <div class="card-more">
        <Link href="/cache">Open Cache</Link>
      </div>
    </Card>
  );
}
