import type { CacheUsage, Telemetry } from '../api/types';
import { RefreshCluster } from '../components/refresh-cluster';
import { useRefresh } from '../refresh/use-refresh';
import type { PageProps } from '../router/routes';
import { refresh } from '../services';
import { ContentHeader } from '../shell/content-header';
import { BoundsCard } from './cache/bounds-card';
import { CleanCard } from './cache/clean-card';
import { CleanupCard } from './cache/cleanup-card';
import { CountersCard } from './cache/counters-card';
import { StageCard } from './cache/stage-card';
import { SwrCard } from './cache/swr-card';

/**
 * The DNS cache, bounded by entries **and** bytes.
 *
 * **Its data lifecycle, in one place.** Two polled endpoints through the shared
 * registry — `/cache` for the stages, the bounds and the counters, `/telemetry`
 * for the SWR and background-cleanup blocks — declared by the route table and
 * acquired by the shell. No timer of this page's own, no event subscription
 * (the socket is closed while it is mounted), and no one-shot on entry: the
 * only request this page issues itself is the clean, and only when the operator
 * presses the button.
 *
 * Leaving releases both subscriptions and stops their timers if no other page
 * wants them.
 */
export function Cache(_props: PageProps) {
  const cache = useRefresh<CacheUsage>(refresh, 'cache');
  const telemetry = useRefresh<Telemetry>(refresh, 'telemetry');

  return (
    <>
      <ContentHeader
        title="DNS cache"
        context={
          <>
            Bounded by entries <b>and</b> bytes —{' '}
            <span class="mono">GET /api/v1/cache</span>
          </>
        }
        cluster={
          <RefreshCluster registry={refresh} endpoint="cache" placement="header" />
        }
      />
      <main class="wrap">
        <StageCard cache={cache.data} />
        <div class="row c2">
          <BoundsCard cache={cache.data} />
          <CountersCard cache={cache.data} />
        </div>
        <div class="row c2 cache-actions">
          <CleanCard cache={cache.data} registry={refresh} />
          <div class="stack">
            <SwrCard telemetry={telemetry.data} registry={refresh} />
            <CleanupCard telemetry={telemetry.data} />
          </div>
        </div>
      </main>
    </>
  );
}

export default Cache;
