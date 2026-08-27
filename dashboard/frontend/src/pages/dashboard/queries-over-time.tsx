import { useMemo, useRef } from 'preact/hooks';
import type uPlot from 'uplot';
import type { HistorySummary } from '../../api/types';
import { useChartTheme } from '../../charts/theme';
import { percent1 } from '../../charts/format';
import { blockedPercent, sumOver } from '../../derive';
import {
  barLabelsPlugin,
  createHoverState,
  hoverPlugin,
  stackedBarsOptions,
} from '../../charts/stacked-bars';
import { Card } from '../../components/card';
import { Chart } from '../../components/chart';
import { EmptyState } from '../../components/empty-state';
import { ErrorState } from '../../components/error-state';
import { RANGES, RANGE_KEYS, type RangeKey } from './ranges';

/**
 * The primary series, and **DNS only** — `history/summary` carries no HTTP
 * data, and this page insists everywhere else that the two pipelines are not
 * conflated.
 *
 * Three non-chart states, which must not be confusable: recording switched off,
 * an empty range, and a failed request. Telling the first two apart is what the
 * page's one `/config` re-read exists for.
 */
export function QueriesOverTime({
  range,
  onRange,
  summary,
  error,
  loading,
  recording,
}: {
  range: RangeKey;
  onRange: (range: RangeKey) => void;
  summary: HistorySummary | null;
  error: Error | null;
  loading: boolean;
  recording: boolean;
}) {
  const theme = useChartTheme();
  const resolution = RANGES[range].resolution;

  // The plot is keyed on this object's identity, so it is built once per
  // `(range, theme)` pair and not once per render — p5-05's finding m4. A
  // `stats` push re-renders this page every ~2 s and must rebuild nothing.
  const items = useRef<HistorySummary | null>(summary);
  items.current = summary;

  const options = useMemo(() => {
    const hover = createHoverState();
    return stackedBarsOptions({
      resolution,
      theme,
      hover,
      plugins: [
        barLabelsPlugin(theme, hover),
        hoverPlugin({
          resolution,
          hover,
          item: (index) => {
            const entry = items.current?.items[index];
            if (entry === undefined) return null;
            return {
              ts: entry.ts,
              queries: entry.queries,
              blocked: entry.blocked,
              // Served per item. Never divided here.
              blockedPercent: entry.blocked_percent,
            };
          },
        }),
      ],
    });
  }, [resolution, theme]);

  const data = useMemo<uPlot.AlignedData>(() => {
    const rows = summary?.items ?? [];
    return [
      rows.map((item) => Date.parse(item.ts) / 1000),
      rows.map((item) => item.queries),
      rows.map((item) => item.blocked),
    ];
  }, [summary]);

  /**
   * The aggregate a bar chart structurally cannot show, and it is summed from
   * **the same items that draw the bars** (R2/R3/R4).
   *
   * Taking it from `/stats` would be wrong at every range: `/stats` is a
   * rolling 24 h snapshot, so at 7 d and 30 d it states the wrong window
   * outright, and even at 24 h it covers a different span from the hour-aligned
   * buckets beneath it — a total that disagrees with its own chart. It follows
   * that this line does **not** move with the `stats` push: the tiles describe
   * the live window, this describes the plotted range.
   */
  const totals = useMemo(() => {
    const rows = summary?.items;
    if (rows === undefined || rows.length === 0) return null;
    const queries = sumOver(rows, (item) => item.queries);
    const blocked = sumOver(rows, (item) => item.blocked);
    return `${queries.toLocaleString()} queries · ${blocked.toLocaleString()} blocked · ${percent1(
      blockedPercent(queries, blocked),
    )} %`;
  }, [summary]);

  // Hidden when recording is off: there is no range to pick.
  const chips = recording ? (
    <span class="chips">
      {RANGE_KEYS.map((key) => (
        <button
          key={key}
          type="button"
          class={key === range ? 'chip on' : 'chip'}
          aria-pressed={key === range}
          onClick={() => onRange(key)}
        >
          {RANGES[key].label}
        </button>
      ))}
    </span>
  ) : undefined;

  return (
    <Card
      title={
        <>
          Queries over time
          {totals !== null && <span class="note ch-aside">{totals}</span>}
        </>
      }
      tools={chips}
      className="z-chart"
    >
      {/* Below 768 px the chips move out of the title bar and become
          full-height rows above the plot: `MobileDashboard.dc.html` draws them
          there because pinch-zoom is not a discoverable gesture. */}
      {chips !== undefined && <div class="chips-mobile">{chips}</div>}
      {body()}
      <div class="chart-legend">
        <span>
          <span class="sw" style={{ background: 'var(--series-permitted)' }} />
          permitted
        </span>
        <span>
          <span class="sw" style={{ background: 'var(--series-blocked)' }} />
          blocked
        </span>
      </div>
      <p class="chart-footnote">
        DNS only — the HTTP pipeline is counted separately and has no
        24 h series.
      </p>
    </Card>
  );

  function body() {
    if (!recording) {
      return (
        <EmptyState title="History is not being recorded">
          Nothing is written to <span class="mono">/data/history</span> while{' '}
          <span class="mono">history.enabled</span> is off. Turn it back on in
          Settings.
        </EmptyState>
      );
    }
    if (error !== null) return <ErrorState error={error} />;
    if (loading && summary === null) return <div class="boot" />;
    if ((summary?.items.length ?? 0) === 0) return <EmptyState />;
    // Decimation is visible. `max_points` is left at the server default, so
    // this is unreachable at the three offered ranges — it is built because
    // `stride` is part of the contract, not because a range trips it.
    const stride = summary?.stride;
    return (
      <Chart
        data={data}
        options={options}
        height={236}
        {...(stride === undefined ? {} : { decimatedBy: stride })}
      />
    );
  }
}
