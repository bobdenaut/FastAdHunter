import { useEffect, useState } from 'preact/hooks';
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
import { RttChart } from './upstreams/rtt-chart';
import { StatesCard } from './upstreams/states-card';
import { useRttHistory } from './upstreams/use-rtt-history';
import { useConfigReader } from './recorded';
import type { RangeKey } from './dashboard/ranges';

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
  const [range, setRange] = useState<RangeKey>('24h');

  // The single-flight reader, not a bare `getConfig`: the mount snapshot and
  // the round-trip chart's disambiguation of an empty range answer are two
  // callers for the same document, and without the join they are two
  // concurrent requests (p5-06's F11).
  const readConfig = useConfigReader();

  useEffect(() => {
    const controller = new AbortController();
    readConfig(controller.signal)
      .then(setConfig)
      // A failed `/config` costs the strategy and nothing else. The counters
      // still render verbatim and the subtitle says what is missing, which is
      // the same stance the Dashboard takes on its own secondary readings.
      .catch(() => undefined);
    return () => controller.abort();
  }, [readConfig]);

  const rtt = useRttHistory(range, config, setConfig, readConfig);

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

        {rtt.recording ? (
          <RttChart
            range={range}
            onRange={setRange}
            history={rtt.history}
            error={rtt.error}
            loading={rtt.loading}
          />
        ) : (
          // Its own state, distinguishable by construction from an empty
          // range: this one is read from `/config`, that one from an answer the
          // recorder actually gave. The endpoint cards above are unaffected —
          // they are live figures, not persisted ones.
          <section class="card">
            <div class="bd">
              <EmptyState title="History is not being recorded">
                Round-trip time over the range needs{' '}
                <span class="mono">/data/history</span>, and nothing is written
                there while <span class="mono">history.enabled</span> is off.
                Turn it back on in Settings.
              </EmptyState>
            </div>
          </section>
        )}

        <div class="row c2">
          <NoPieCard />
          <StatesCard mode={mode} />
        </div>
      </main>
    </>
  );
}

export default Upstreams;
