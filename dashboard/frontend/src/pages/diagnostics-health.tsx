import { useEffect, useState } from 'preact/hooks';
import { getConfig } from '../api/config';
import type { Config, Health, ListsResponse, Telemetry } from '../api/types';
import { Card } from '../components/card';
import { DegradedBanner } from '../components/degraded-banner';
import { RefreshCluster } from '../components/refresh-cluster';
import { StatusPill } from '../components/status-pill';
import { upstreamMode } from '../derive';
import { useRefresh } from '../refresh/use-refresh';
import type { PageProps } from '../router/routes';
import { refresh } from '../services';
import { ContentHeader } from '../shell/content-header';
import { formatUptime } from '../time';
import { BackpressureCard } from './health/backpressure-card';
import { EngineCard } from './health/engine-card';
import { NeverCard } from './health/never-card';
import { OutcomesCard } from './health/outcomes-card';
import { RuleListsCard } from './health/rule-lists-card';
import { EndpointSummary } from './health/endpoint-summary';

/**
 * Is it serving, and what has gone wrong.
 *
 * **Its data lifecycle, in one place.** Three polled endpoints through the
 * shared registry — `/health` for the status, `/telemetry` for the counters
 * that explain it, `/lists` for the problem summary — and one `GET /config` on
 * mount for the upstream strategy, which is boot-only and cannot change under a
 * running process. No event type, so the socket is closed throughout, and no
 * timer of this page's own: three refresh clusters, one per polled endpoint.
 *
 * Every counter here is process-lifetime. `/history/perf` persists the answer
 * deltas per interval, and this page says so rather than fetching them: a
 * history read on a live-status screen would be a second window with a second
 * meaning on one card.
 */
export function DiagnosticsHealth(_props: PageProps) {
  const health = useRefresh<Health>(refresh, 'health');
  const telemetry = useRefresh<Telemetry>(refresh, 'telemetry');
  const lists = useRefresh<ListsResponse>(refresh, 'lists');
  const [config, setConfig] = useState<Config | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    getConfig(controller.signal)
      .then(setConfig)
      // A failed `/config` costs the strategy and nothing else; the summary
      // degrades to a count without states, which is what it means.
      .catch(() => undefined);
    return () => controller.abort();
  }, []);

  const mode = upstreamMode(config?.dns.upstreams.strategy);
  const status = health.data?.status ?? null;

  return (
    <>
      <ContentHeader
        title="Health"
        context={
          <>
            Is it serving, and what has gone wrong —{' '}
            <span class="mono">GET /health</span> plus the counters that explain
            it
          </>
        }
      />
      <main class="wrap">
        <Card
          title={
            status === null ? 'Serving' : <>Serving — status {status}</>
          }
          tools={
            <RefreshCluster registry={refresh} endpoint="health" />
          }
          className="health-status"
        >
          <div class="health-head">
            <div class="health-figure">
              <div class="figure mono">
                {health.data === null
                  ? '—'
                  : formatUptime(health.data.uptime_seconds)}
              </div>
              <p class="note">
                uptime
                {health.data !== null && (
                  <>
                    {' · '}
                    <span class="mono">v{health.data.version}</span>
                  </>
                )}
              </p>
            </div>
            {status !== null && <StatusPill status={status} />}
          </div>

          {status === 'degraded' && <DegradedBanner mode={mode} />}

          <EndpointSummary
            mode={mode}
            upstreams={telemetry.data?.upstreams ?? null}
          />
        </Card>

        <div class="row c2">
          <OutcomesCard telemetry={telemetry.data} />
          <BackpressureCard telemetry={telemetry.data} />
        </div>

        <RuleListsCard lists={lists.data} />

        <EngineCard telemetry={telemetry.data} />

        <NeverCard />
      </main>
    </>
  );
}

export default DiagnosticsHealth;
