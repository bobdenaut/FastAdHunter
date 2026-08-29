import { useMemo } from 'preact/hooks';
import type uPlot from 'uplot';
import type { HistoryPerf, PerfLatency } from '../../api/types';
import { epochSeconds, msAxisLabel } from '../../charts/format';
import { lineChartOptions } from '../../charts/lines';
import { useChartTheme } from '../../charts/theme';
import { Card } from '../../components/card';
import { Chart } from '../../components/chart';
import { EmptyState } from '../../components/empty-state';
import { ErrorState } from '../../components/error-state';
import { latencyMs } from '../../derive';
import { RANGES, RANGE_KEYS, type RangeKey } from '../dashboard/ranges';
import { STAGE_BUDGET_LABEL, STAGE_BUDGET_MS } from './budgets';
import { AxisEnds } from './axis-ends';

interface StageSeries {
  key: keyof PerfLatency;
  label: string;
  colour: (theme: { series1: string; series2: string; series5: string }) => string;
  dash: boolean;
}

/**
 * Six series, three stages, two percentiles each — and **never an average**. An
 * average hides the tail the budget is written against, and a fast one is
 * usually just a high cache-hit rate rather than a fast engine.
 */
const SERIES: readonly StageSeries[] = [
  { key: 'block_p99', label: 'block p99', colour: (t) => t.series2, dash: false },
  { key: 'block_p50', label: 'block p50', colour: (t) => t.series2, dash: true },
  { key: 'cache_hit_p99', label: 'cache hit p99', colour: (t) => t.series1, dash: false },
  { key: 'cache_hit_p50', label: 'cache hit p50', colour: (t) => t.series1, dash: true },
  { key: 'forward_p99', label: 'forward p99', colour: (t) => t.series5, dash: false },
  { key: 'forward_p50', label: 'forward p50', colour: (t) => t.series5, dash: true },
];

export function LatencyChart({
  range,
  onRange,
  history,
  error,
  loading,
}: {
  range: RangeKey;
  onRange: (range: RangeKey) => void;
  history: HistoryPerf | null;
  error: Error | null;
  loading: boolean;
}) {
  const theme = useChartTheme();

  // Keyed on the theme alone: the range changes the data, not the plot, so a
  // range selection pushes new readings into the existing instance rather than
  // rebuilding it (p5-05's finding m4).
  const options = useMemo(
    () =>
      lineChartOptions({
        theme,
        series: SERIES.map((series) => ({
          label: series.label,
          colour: series.colour(theme),
          ...(series.dash ? { dash: true } : {}),
        })),
        format: (value) => (value === 0 ? null : msAxisLabel(value)),
        budget: { value: STAGE_BUDGET_MS, label: STAGE_BUDGET_LABEL },
      }),
    [theme],
  );

  const data = useMemo<uPlot.AlignedData>(() => {
    const rows = history?.items ?? [];
    return [
      rows.map((item) => epochSeconds(item.ts)),
      // E12 — seconds to milliseconds, with an exact `0.0` mapped to a `null`
      // gap. `spanGaps` is off, so a stage that saw no traffic breaks its line
      // rather than being bridged into a dip it never had.
      ...SERIES.map((series) =>
        rows.map((item) => {
          const latency = item.latency;
          return latency === undefined ? null : latencyMs(latency[series.key]);
        }),
      ),
    ] as uPlot.AlignedData;
  }, [history]);

  const chips = (
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
  );

  return (
    <Card title="In-engine latency, p50 and p99 by stage" tools={chips}>
      <div class="chips-mobile">{chips}</div>
      {body()}
      <div class="chart-legend latency-legend">
        <span>
          <span class="sw" style={{ background: 'var(--series-2)' }} />
          block
        </span>
        <span>
          <span class="sw" style={{ background: 'var(--series-1)' }} />
          cache hit
        </span>
        <span>
          <span class="sw" style={{ background: 'var(--series-5)' }} />
          forward (engine only)
        </span>
        <span class="note">solid p99 · dashed p50</span>
        <span class="note legend-aside">
          never an average — an average hides the tail the budget is written
          against
        </span>
      </div>
      <p class="chart-footnote">
        Forward excludes upstream round-trip time: these three stages measure
        only what FastAdHunter adds. Block and cache-hit sit so near the axis
        that they read as flat — which is the point of the chart, not a failure
        of it.
        <span class="footnote-line">
          Percentiles are bucket-granularity estimates over each sampling
          interval and saturate at the top finite bucket. Good for a trend line,
          not exact quantiles.
        </span>
      </p>
    </Card>
  );

  function body() {
    if (error !== null) return <ErrorState error={error} />;
    const empty = (history?.items.length ?? 0) === 0;
    if (loading && empty) return <div class="boot" />;
    if (empty) return <EmptyState />;
    const stride = history?.stride;
    return (
      <Chart
        data={data}
        options={options}
        height={226}
        footer={<AxisEnds items={history?.items ?? []} />}
        {...(stride === undefined ? {} : { decimatedBy: stride })}
      />
    );
  }
}
