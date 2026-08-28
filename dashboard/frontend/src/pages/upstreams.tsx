import { useEffect, useState } from 'preact/hooks';
import { getConfig } from '../api/config';
import type { Config, Health, Telemetry } from '../api/types';
import { Card } from '../components/card';
import { EmptyState } from '../components/empty-state';
import { RefreshCluster } from '../components/refresh-cluster';
import { upstreamMode } from '../derive';
import { useRefresh } from '../refresh/use-refresh';
import type { PageProps } from '../router/routes';
import { refresh } from '../services';
import { ContentHeader } from '../shell/content-header';
import { DegradedBanner } from '../components/degraded-banner';
import { EndpointRow } from './upstreams/endpoint-row';
import { NoPieCard } from './upstreams/no-pie-card';
import { StatesCard } from './upstreams/states-card';

/**
 * Health and state of each configured server.
 *
 * **The strategy is the key to the whole page**, and it is only on `/config`:
 * `telemetry.upstreams[]` does not carry it, and without it the health block
 * cannot be read at all. Under `fallback` every row publishes `state: healthy`,
 * `penalty_round: 0` and zeros for penalties, probes and penalized seconds,
 * which API.md is explicit means "no health state exists to report" and not
 * "everything is fine". Showing those zeros without naming the strategy states
 * the opposite of the truth, so this page names it or says it could not.
 *
 * **Its data lifecycle, in one place.** Two polled endpoints through the shared
 * registry (`/telemetry`, `/health`) and one `/config` read on mount — no
 * re-read, because `dns.upstreams.strategy` is boot-only and cannot change
 * under a running process. No event type, so the socket is closed throughout.
 */
export function Upstreams(_props: PageProps) {
  const telemetry = useRefresh<Telemetry>(refresh, 'telemetry');
  const health = useRefresh<Health>(refresh, 'health');
  const [config, setConfig] = useState<Config | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    getConfig(controller.signal)
      .then(setConfig)
      // A failed `/config` costs the strategy and nothing else. The counters
      // still render verbatim and the subtitle says what is missing, which is
      // the same stance the Dashboard takes on its own secondary readings.
      .catch(() => undefined);
    return () => controller.abort();
  }, []);

  const mode = upstreamMode(config?.dns?.upstreams?.strategy);
  const upstreams = telemetry.data?.upstreams ?? null;
  const unknownFamily =
    upstreams !== null && upstreams.some((row) => row.family === null);

  return (
    <>
      <ContentHeader
        title="Upstream endpoints"
        context={
          <>
            Health and state of each configured server —{' '}
            <span class="mono">/api/v1/telemetry</span>{' '}
            <span class="mono">upstreams</span>,{' '}
            {mode === 'unknown' ? (
              <>strategy unknown — configuration unreachable</>
            ) : (
              <>
                strategy <span class="mono">{mode}</span>
              </>
            )}
          </>
        }
        cluster={
          <RefreshCluster
            registry={refresh}
            endpoint="health"
            placement="header"
          />
        }
      />
      <main class="wrap">
        {health.data?.status === 'degraded' && <DegradedBanner mode={mode} />}

        <Card
          title="Endpoints"
          tools={
            <RefreshCluster
              registry={refresh}
              endpoint="telemetry"
              secondary="order is the configured order — the index is what a query reports as its answering endpoint"
            />
          }
          bodyClass="upstreams-body"
        >
          {upstreams === null ? (
            <EmptyState title="Not read yet" />
          ) : upstreams.length === 0 ? (
            <EmptyState title="No upstream endpoints are configured" />
          ) : (
            <>
              {upstreams.map((upstream, index) => (
                <EndpointRow
                  key={upstream.address}
                  index={index}
                  upstream={upstream}
                  mode={mode}
                />
              ))}
              <p class="note ep-foot">
                Counters are cumulative since process start.{' '}
                <b>Consecutive failures is the live one</b> — it resets on a
                success, so it says what is happening now, where the totals say
                what has happened ever.
                <span class="footnote-line">
                  The failure-run histogram counts closed runs by length. Many
                  short runs is ordinary packet loss; a few long ones is an
                  endpoint that goes away and comes back, which is what earns a
                  penalty.
                </span>
                {unknownFamily && (
                  <span class="footnote-line">
                    An endpoint reads <span class="mono">family unknown</span>{' '}
                    when its host is a domain name resolved at connect time —
                    nothing resolves it to fill the field in.
                  </span>
                )}
              </p>
            </>
          )}
        </Card>

        <div class="row c2">
          <NoPieCard />
          <StatesCard mode={mode} />
        </div>
      </main>
    </>
  );
}

export default Upstreams;
