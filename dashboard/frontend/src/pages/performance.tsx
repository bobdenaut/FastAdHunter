import { useEffect, useState } from 'preact/hooks';
import type { Config } from '../api/types';
import { EmptyState } from '../components/empty-state';
import type { PageProps } from '../router/routes';
import { ContentHeader } from '../shell/content-header';
import type { RangeKey } from './dashboard/ranges';
import { useConfigReader } from './recorded';
import { LatencyChart } from './performance/latency-chart';
import { QpsCard } from './performance/qps-card';
import { ReadingCard } from './performance/reading-card';
import { StageTiles } from './performance/stage-tiles';
import { VerdictsCard } from './performance/verdicts-card';
import { usePerfHistory } from './performance/use-perf-history';

/** What one persisted row covers when `/config` did not answer. It is the
 *  documented default of `history.sample_interval_seconds`, stated as a
 *  fallback rather than assumed as a fact. */
const DEFAULT_SAMPLE_SECONDS = 60;

/**
 * Latency by stage, per sample. Two of the three stages are what the engine
 * adds; `forward` is the whole round trip and says so on its own tile.
 *
 * **Its data lifecycle, in one place.** This page polls nothing and holds no
 * timer: `/history/perf` is a range query issued once per range selection, and
 * `/config` is one read on mount (plus at most one re-read, and only when an
 * empty response makes the recorder's state ambiguous). It declares no polled
 * endpoint and no event type, so the socket is closed and the connection
 * indicator reads `not needed here` for as long as it is mounted. Parked on,
 * it produces no requests at all.
 */
export function Performance(_props: PageProps) {
  const [config, setConfig] = useState<Config | null>(null);
  const [range, setRange] = useState<RangeKey>('24h');

  // One reader for both the mount snapshot and the disambiguation re-read
  // (p5-06's F11): the join lives in `pages/recorded.ts`, shared with the
  // Dashboard.
  const readConfig = useConfigReader();

  useEffect(() => {
    const controller = new AbortController();
    readConfig(controller.signal)
      .then(setConfig)
      // A failed `/config` costs this page two secondary readings — whether the
      // recorder is on, and how long one row covers — and nothing else, so it
      // is a degraded rendering rather than an error state for the whole page.
      .catch(() => undefined);
    return () => controller.abort();
  }, [readConfig]);

  const perf = usePerfHistory(range, config, setConfig, readConfig);
  const seconds =
    config?.history?.sample_interval_seconds ?? DEFAULT_SAMPLE_SECONDS;

  return (
    <>
      <ContentHeader
        title="Performance"
        context={
          <>
            Latency by stage, per sample —{' '}
            <span class="mono">GET /api/v1/history/perf</span>, one row per{' '}
            {seconds} s
          </>
        }
      />
      <main class="wrap">
        {perf.recording ? (
          <>
            <StageTiles items={perf.history?.items ?? []} error={perf.error} />
            <LatencyChart
              range={range}
              onRange={setRange}
              history={perf.history}
              error={perf.error}
              loading={perf.loading}
            />
            <div class="row c2">
              <QpsCard
                history={perf.history}
                error={perf.error}
                loading={perf.loading}
              />
              <VerdictsCard
                history={perf.history}
                error={perf.error}
                loading={perf.loading}
              />
            </div>
          </>
        ) : (
          // R5 — its own state, and distinguishable by construction from an
          // empty range: this one is read from `/config`, that one from an
          // answer the recorder actually gave.
          <section class="card">
            <div class="bd">
              <EmptyState title="History is not being recorded">
                Nothing is written to <span class="mono">/data/history</span>{' '}
                while <span class="mono">history.enabled</span> is off. Turn it
                back on in Settings.
              </EmptyState>
            </div>
          </section>
        )}
        <ReadingCard />
      </main>
    </>
  );
}

export default Performance;
