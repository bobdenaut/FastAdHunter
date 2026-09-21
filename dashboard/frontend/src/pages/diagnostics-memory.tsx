import { useEffect, useState } from 'preact/hooks';
import { getDebugMemory } from '../api/debug';
import { getTelemetry } from '../api/telemetry';
import { formatMiB } from '../charts/format';
import { MEMORY_PERF_FIELDS, getHistoryPerf } from '../api/history';
import type { DebugMemory, HistoryPerf, Telemetry } from '../api/types';
import { EmptyState } from '../components/empty-state';
import { DataAge } from '../components/data-age';
import { nowMs } from '../lifecycle/timers';
import { median, overAccounted, sinceLastRestart, windowTrend } from '../derive';
import { ErrorState } from '../components/error-state';
import type { PageProps } from '../router/routes';
import { ContentHeader } from '../shell/content-header';
import { Icon } from '../shell/icon';
import { AllocatorCard } from './memory/allocator-card';
import { CompositionCard } from './memory/composition-card';
import { FaultsCard } from './memory/faults-card';
import { KeyMetricsCard } from './memory/key-metrics-card';
import { KpiRail } from './memory/kpi-rail';
import { TrendCard } from './memory/trend-card';
import { RANGES, type RangeKey } from './dashboard/ranges';
import { STEADY_STATE_BUDGET } from './memory/budgets';

/**
 * How many points each range asks for.
 *
 * **24 h asks for one point per sample, not the endpoint's default.** The
 * default is 1 000 against 1 440 sixty-second samples, so the reader would
 * decimate to a stride of 2 and the chart would silently be drawing every other
 * minute while the page said otherwise. 1 440 is well inside the 5 000 cap and
 * makes "every sample" true.
 *
 * 7 d and 30 d cannot reach stride 1 — 10 080 and 43 200 rows — so they take the
 * cap. Decimation keeps whole rows rather than averaging them, so a smaller
 * stride is strictly less blindness: what it costs is a transient shorter than
 * the stride, which is the gap `peak_rss` exists to cover.
 */
const MAX_POINTS: Record<RangeKey, number> = {
  '24h': 1_440,
  '7d': 5_000,
  '30d': 5_000,
};

/**
 * Where RSS goes, now and over time.
 *
 * **Its data lifecycle, in one place.** One `GET /api/v1/debug/memory` on entry
 * for the instant, and one `GET /api/v1/history/perf` per range selection for
 * the trend — a range query, not a poll. This page declares no polled endpoint
 * and no event type, so it holds no timer, the socket is closed while it is the
 * active route, and parked on it the page produces no requests at all.
 *
 * `/debug/memory` is deliberately not a `REFRESH_ENDPOINTS` member: adding it
 * would give a diagnostic endpoint a standing timer on every page that happened
 * to declare it.
 */
export function DiagnosticsMemory(_props: PageProps) {
  const [memory, setMemory] = useState<DebugMemory | null>(null);
  const [memoryError, setMemoryError] = useState<Error | null>(null);
  const [history, setHistory] = useState<HistoryPerf | null>(null);
  const [historyError, setHistoryError] = useState<Error | null>(null);
  const [loading, setLoading] = useState(true);
  const [range, setRange] = useState<RangeKey>('24h');
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [readAt, setReadAt] = useState<number | null>(null);
  const [reloads, setReloads] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    getDebugMemory(controller.signal)
      .then((response) => {
        setMemory(response);
        setReadAt(nowMs());
      })
      .catch((cause: unknown) => {
        if (cause instanceof Error && cause.name === 'AbortError') return;
        setMemoryError(cause instanceof Error ? cause : new Error(String(cause)));
      });
    return () => controller.abort();
  }, [reloads]);

  // `/telemetry` for the two Key-metrics rows `/debug/memory` does not carry —
  // the compiled rule count and uptime. One shot on entry, like the other two:
  // this page adds no timer and no subscription, which is the invariant, and a
  // one-shot fetch is not a poll.
  useEffect(() => {
    const controller = new AbortController();
    getTelemetry(controller.signal)
      .then(setTelemetry)
      // A failure here costs two rows, never the page: they render an em dash.
      .catch(() => undefined);
    return () => controller.abort();
  }, [reloads]);

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setHistoryError(null);
    getHistoryPerf(
      {
        from: new Date(Date.now() - RANGES[range].spanMs).toISOString(),
        fields: MEMORY_PERF_FIELDS,
        maxPoints: MAX_POINTS[range],
      },
      controller.signal,
    )
      .then((response) => {
        setHistory(response);
        setLoading(false);
      })
      .catch((cause: unknown) => {
        if (cause instanceof Error && cause.name === 'AbortError') return;
        setHistoryError(cause instanceof Error ? cause : new Error(String(cause)));
        setLoading(false);
      });
    return () => controller.abort();
    // `reloads` as well as `range`: Refresh has to move the whole page. The
    // trend chart, the four KPI sparklines, the allocator's window min/max and
    // the residual verdict all read this response, and the header's age covers
    // them too — refreshing only the instant figures would leave four cards on
    // the history fetched on entry under a timestamp that says otherwise.
  }, [range, reloads]);

  const safeMemory = memoryError === null ? memory : null;
  const rss = safeMemory?.process_rss ?? null;

  return (
    <>
      <ContentHeader
        title="Memory"
        glyph="memory"
        context={
          <>
            Where RSS goes, now and over time —{' '}
            <span class="mono">GET /api/v1/debug/memory</span> for the instant,{' '}
            <span class="mono">GET /api/v1/history/perf</span> for the trend.
            <br />
            Budgets are decimal <b>MB</b>, readings are <b>MiB</b>:{' '}
            {/* The worked example is computed from the reading on screen, not
                written into the copy. The artboard prints its own RSS here; a
                hard-coded pair would be a figure nobody measured, and would go
                stale against the number in the card below it. Falls back to the
                budget, which is a documented constant and true either way. */}
            {rss === null
              ? `${String(STEADY_STATE_BUDGET / 1_000_000)} MB is ${formatMiB(STEADY_STATE_BUDGET)}`
              : `${formatMiB(rss)} is ${(rss / 1_000_000).toFixed(1)} MB`}
            , and that ~4.9 % gap is where a false breach comes from.
          </>
        }
        actions={
          <>
            <ResidualVerdict history={historyError === null ? history : null} />
            <span class="hd-updated">
              <DataAge fetchedAt={readAt} prefix />
            </span>
            <button
              type="button"
              class="hd-refresh"
              onClick={() => setReloads((count) => count + 1)}
              aria-label="Refresh"
              title="Refresh"
            >
              <Icon name="refresh" size={15} />
            </button>
          </>
        }
      />
      <main class="wrap memory-page">
        {memoryError !== null ? (
          <ErrorState error={memoryError} />
        ) : safeMemory === null ? null : rss === null ? (
          <EmptyState title="RSS unavailable on this platform">
            <span class="mono">process_rss</span> is read from{' '}
            <span class="mono">/proc/self/status</span> and is null off Linux, so
            there is no total to draw a footprint against.
          </EmptyState>
        ) : (
          <KpiRail
            memory={safeMemory}
            items={historyError === null ? (history?.items ?? []) : []}
            rss={rss}
            range={range}
          />
        )}

        <div class="row c4 memory-row-4">
          <CompositionCard
            memory={safeMemory}
            rules={telemetry?.ruleset.rules ?? null}
          />
          <KeyMetricsCard memory={safeMemory} telemetry={telemetry} />
          <FaultsCard
            history={historyError === null ? history : null}
            range={range}
          />
          <AllocatorCard
            memory={safeMemory}
            history={historyError === null ? history : null}
            range={range}
          />
        </div>

        <TrendCard
          history={history}
          error={historyError}
          loading={loading}
          range={range}
          onRange={setRange}
        />
      </main>
    </>
  );
}

/**
 * The one-word answer the whole page is built to support.
 *
 * **A leak is residual that rises and never comes back**, so the verdict is a
 * statement about the *shape* of the window, not about the current value —
 * residual is never zero, and a large residual is not a fault. It compares the
 * first and last thirds of the window and says `rising` only when the trend
 * holds across it.
 *
 * It is deliberately not a status colour when it is fine: "stable" is the
 * ordinary state of this page, and a green badge on every visit trains a reader
 * to stop looking at it.
 *
 * **A window too short to have a shape gets its own neutral state, not the
 * stable one and not an empty header.** The artboard carries this badge on
 * every visit, and a slot that disappears is a slot a reader stops looking for;
 * but "stable" on two samples would be a claim the data cannot support. So the
 * third state says exactly what is true — there is not enough history yet — in
 * the neutral tone, which is neither a pass nor a warning.
 *
 * **Only the newest process is read.** "Rises and never comes back" is a claim
 * about one lifetime, so rows before the last restart are dropped rather than
 * compared against. A 7 d window over a device redeployed twice that week
 * otherwise reports `rising` off three binaries' data, at full confidence, on
 * the visit right after a deploy — which is exactly when someone is looking. A
 * fresh process then has too few rows to have a shape, and says so.
 */
function ResidualVerdict({ history }: { history: HistoryPerf | null }) {
  const values = sinceLastRestart(history?.items ?? [])
    .filter((item) => !overAccounted(item))
    .map((item) => item.memory?.residual_bytes)
    .filter((value): value is number => value !== undefined);
  // 10 % of the opening level, so a flat series with allocator jitter does not
  // trip it and a genuine climb does.
  const trend = windowTrend(values, 0.1, median);

  if (trend === null) {
    return (
      <span class="pill neutral memory-verdict">
        <span class="dot" />
        residual — not enough history
      </span>
    );
  }

  const rising = trend === 'rising';
  return (
    <span class={rising ? 'pill warn memory-verdict' : 'pill good memory-verdict'}>
      <span class="dot" />
      {rising ? 'residual rising' : 'residual stable'}
    </span>
  );
}

export default DiagnosticsMemory;
