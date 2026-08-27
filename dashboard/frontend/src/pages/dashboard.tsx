import { useCallback, useEffect, useRef, useState } from 'preact/hooks';
import { getConfig } from '../api/config';
import { getHistorySummary } from '../api/history';
import { getStats } from '../api/stats';
import type {
  CacheUsage,
  ClientsResponse,
  Config,
  Health,
  HistorySummary,
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

  useEffect(() => {
    const controller = new AbortController();
    getConfig(controller.signal)
      .then(setConfig)
      // `/config` failing costs the page two secondary readings — the history
      // state and the upstream strategy — and nothing else, so it is not an
      // error state for the whole page.
      .catch(() => undefined);
    return () => controller.abort();
  }, []);

  const history = useHistory(range, config, setConfig);

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
        summary={history.summary}
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
          summary={history.summary}
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

export interface HistoryState {
  summary: HistorySummary | null;
  error: Error | null;
  loading: boolean;
  /** `false` only once `/config` has actually said so. */
  recording: boolean;
}

/**
 * The range query, and the one place the two empty states are told apart.
 *
 * `history.enabled` is runtime-mutable and this page does not subscribe to
 * `config_changed` (it renders nothing such an event would change), so a mount
 * snapshot can go stale in exactly one way that matters: Settings switches
 * recording off in another tab and `/history/*` then answers `200` with empty
 * `items` for ever. An empty response with a snapshot of `enabled: true` is
 * therefore worth **one** extra `/config` read before choosing between "history
 * disabled" and "no data in this range" — a one-shot triggered by a specific
 * response, not a poll.
 */
function useHistory(
  range: RangeKey,
  config: Config | null,
  setConfig: (config: Config) => void,
): HistoryState {
  const [summary, setSummary] = useState<HistorySummary | null>(null);
  const [error, setError] = useState<Error | null>(null);
  const [loading, setLoading] = useState(true);
  const [recheck, setRecheck] = useState(false);

  const snapshotEnabled = config?.history?.enabled ?? true;
  const enabledRef = useRef(snapshotEnabled);
  enabledRef.current = snapshotEnabled;

  const disambiguate = useCallback(
    (signal: AbortSignal) => {
      if (!enabledRef.current) return;
      setRecheck(true);
      getConfig(signal)
        .then((fresh) => {
          setConfig(fresh);
          setRecheck(false);
        })
        .catch(() => setRecheck(false));
    },
    [setConfig],
  );

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    getHistorySummary(rangeQuery(range, Date.now()), controller.signal)
      .then((response) => {
        setSummary(response);
        setLoading(false);
        if (response.items.length === 0) disambiguate(controller.signal);
      })
      .catch((error: unknown) => {
        if (error instanceof Error && error.name === 'AbortError') return;
        setError(error instanceof Error ? error : new Error(String(error)));
        setLoading(false);
      });
    return () => controller.abort();
  }, [range, disambiguate]);

  return {
    summary,
    error,
    // The chart holds its loading state across the disambiguation round trip
    // rather than flashing the wrong empty state for one frame.
    loading: loading || recheck,
    recording: snapshotEnabled,
  };
}

export default Dashboard;
