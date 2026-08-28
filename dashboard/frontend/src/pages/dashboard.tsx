import { useCallback, useEffect, useState } from 'preact/hooks';
import { getHistorySummary } from '../api/history';
import { getStats } from '../api/stats';
import type {
  CacheUsage,
  ClientsResponse,
  Config,
  Health,
  ListsResponse,
  Stats,
  Telemetry,
} from '../api/types';
import { ErrorState } from '../components/error-state';
import { useRefresh } from '../refresh/use-refresh';
import type { PageProps } from '../router/routes';
import { refresh, socket } from '../services';
import { CacheState } from './dashboard/cache-state';
import { DnsTiles, HttpTiles } from './dashboard/tiles';
import { QueriesOverTime } from './dashboard/queries-over-time';
import { QueryTypes } from './dashboard/query-types';
import { RulesetCard } from './dashboard/ruleset-card';
import { TopClients } from './dashboard/top-clients';
import { TopDomains } from './dashboard/top-domains';
import { UpstreamHealth } from './dashboard/upstream-health';
import { rangeQuery, type RangeKey } from './dashboard/ranges';
import { useConfigReader, useRecordedRange } from './recorded';

/**
 * The landing page, and the one that proves the shell: tiles, a chart, a donut,
 * top-N tables and the live stats push together.
 *
 * **Its data lifecycle, in one place.** On mount it issues three one-shots —
 * `/stats`, `/config` and `/history/summary` — and subscribes to five polled
 * endpoints through the shared registry, which the route table declares and the
 * shell acquires. Nothing here starts a timer: `/stats` is never polled because
 * the socket's `stats` frame *is* the refresh, and `/history/summary` is a range
 * query refetched only when the operator changes the range.
 *
 * Leaving the page releases every subscription and aborts every in-flight
 * request, which is the invariant the phase is measured against.
 */
export function Dashboard(_props: PageProps) {
  const [stats, setStats] = useState<Stats | null>(null);
  const [statsError, setStatsError] = useState<Error | null>(null);
  const [config, setConfig] = useState<Config | null>(null);
  const [range, setRange] = useState<RangeKey>('24h');

  const telemetry = useRefresh<Telemetry>(refresh, 'telemetry');
  const cache = useRefresh<CacheUsage>(refresh, 'cache');
  const health = useRefresh<Health>(refresh, 'health');
  const clients = useRefresh<ClientsResponse>(refresh, 'clients');
  const lists = useRefresh<ListsResponse>(refresh, 'lists');

  // `GET /stats` exists only because the first push is up to ~2 s away. After
  // it, the socket is the refresh and this endpoint is never called again.
  useEffect(() => {
    const controller = new AbortController();
    getStats(controller.signal)
      .then(setStats)
      .catch((error: unknown) => {
        if (error instanceof Error && error.name === 'AbortError') return;
        setStatsError(error instanceof Error ? error : new Error(String(error)));
      });
    return () => controller.abort();
  }, []);

  // The push is byte-for-byte the `GET /stats` payload, so it replaces the
  // whole snapshot rather than patching it.
  useEffect(
    () => socket.on('stats', (data) => setStats(data as unknown as Stats)),
    [],
  );

  // One reader for the mount snapshot and the disambiguation re-read (p5-06's
  // F11, closed): the single-flight join lives in `pages/recorded.ts`, shared
  // with Performance.
  const readConfig = useConfigReader();

  useEffect(() => {
    const controller = new AbortController();
    readConfig(controller.signal)
      .then(setConfig)
      // `/config` failing costs the page two secondary readings — the history
      // state and the upstream strategy — and nothing else, so it is not an
      // error state for the whole page.
      .catch(() => undefined);
    return () => controller.abort();
  }, [readConfig]);

  /**
   * `useRecordedRange` over `/history/summary` — the protocol (one request per
   * range selection, the `/config` disambiguation of an empty answer, the
   * three failure paths) is `pages/recorded.ts`, shared with Performance.
   */
  const readSummary = useCallback(
    (now: number, signal: AbortSignal) =>
      getHistorySummary(rangeQuery(range, now), signal),
    [range],
  );
  const history = useRecordedRange(readSummary, config, setConfig, readConfig);

  // R12 — the count of enabled lists, which `/telemetry.ruleset` does not
  // carry.
  const enabledLists =
    lists.data === null
      ? null
      : lists.data.items.filter((item) => item.enabled).length;

  if (statsError !== null && stats === null) {
    return <ErrorState error={statsError} />;
  }

  return (
    <>
      <DnsTiles
        stats={stats}
        clientCount={clients.data === null ? null : clients.data.items.length}
      />
      <HttpTiles
        telemetry={telemetry.data}
        status={health.data?.status ?? null}
        enabledLists={enabledLists}
      />
      <QueriesOverTime
        range={range}
        onRange={setRange}
        summary={history.data}
        error={history.error}
        loading={history.loading}
        recording={history.recording}
      />
      {/* One grid rather than three rows of two. The pairs land exactly as the
          artboard draws them, and on a phone — where they become one column —
          `order` can put them in `MobileDashboard.dc.html`'s sequence and drop
          the three that artboard drops. */}
      <div class="row c2 z-cards">
        <QueryTypes
          range={range}
          summary={history.data}
          recording={history.recording}
          loading={history.loading}
          className="z-query-types"
        />
        <UpstreamHealth
          telemetry={telemetry.data}
          strategy={config?.dns?.upstreams?.strategy ?? null}
          registry={refresh}
          className="z-upstreams"
        />
        <TopDomains
          title="Top queried domains"
          domains={stats?.top_queried_domains}
          className="z-top-queried"
        />
        <TopDomains
          title="Top blocked domains"
          domains={stats?.top_blocked_domains}
          tone="blocked"
          className="z-top-blocked"
        />
        <TopClients
          clients={clients.data?.items}
          registry={refresh}
          className="z-top-clients"
        />
        <CacheState
          cache={cache.data}
          registry={refresh}
          className="z-cache"
        />
      </div>
      <div class="row z-ruleset">
        <RulesetCard
          telemetry={telemetry.data}
          enabledLists={enabledLists}
          registry={refresh}
        />
      </div>
      <p class="note z-phone-note">
        Upstream health, ruleset detail and the long-range tables are not
        dropped on a phone — they are one tap away on their own pages. The
        Dashboard keeps what answers “is it working right now”.
      </p>
    </>
  );
}

export default Dashboard;
