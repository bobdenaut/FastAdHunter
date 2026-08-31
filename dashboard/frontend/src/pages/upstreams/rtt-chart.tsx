import { useMemo } from 'preact/hooks';
import type uPlot from 'uplot';
import type { HistoryPerf, PerfItem } from '../../api/types';
import { epochSeconds, msAxisLabel } from '../../charts/format';
import { lineChartOptions } from '../../charts/lines';
import { useChartTheme, type ChartTheme } from '../../charts/theme';
import { Card } from '../../components/card';
import { Chart } from '../../components/chart';
import { EmptyState } from '../../components/empty-state';
import { ErrorState } from '../../components/error-state';
import { RANGES, RANGE_KEYS, type RangeKey } from '../dashboard/ranges';
import { AxisEnds } from '../performance/axis-ends';

/**
 * How many endpoints the chart draws. Two series each, and a plot past this
 * many lines stops being readable — the endpoint cards above carry every
 * configured server either way, so the cut is to the chart and not to the
 * page. Configured order, which is the order the cards are in.
 */
const MAX_SERIES = 4;

/** One colour per endpoint, in configured order, wrapping past the fourth —
 *  the same four the other pages' series draw from. */
function colourFor(theme: ChartTheme, index: number): string {
  const palette = [theme.series1, theme.series2, theme.series5, theme.series4];
  return palette[index % palette.length] ?? theme.series1;
}

/**
 * Round-trip time per endpoint over the persisted range — the one figure on a
 * `/history/perf` row that is per-interval rather than cumulative.
 *
 * **Answered attempts only.** A timed-out attempt is a failure and is counted
 * as one; timing it would pin every percentile at `timeout_ms` for as long as
 * an endpoint stayed down, which says nothing about how fast the ones that
 * answer are. An endpoint that answered nothing in an interval breaks its line
 * rather than drawing a zero.
 */
export function RttChart({
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
  const addresses = useMemo(
    () => endpointOrder(history?.items ?? []),
    [history],
  );

  // Keyed on the theme and the endpoint list: the range changes the data, not
  // the plot, but a config reload that renames an endpoint changes the series
  // and does need a rebuild (p5-05's finding m4).
  const options = useMemo(
    () =>
      lineChartOptions({
        theme,
        series: addresses.flatMap((address, index) => {
          const colour = colourFor(theme, index);
          return [
            { label: `${address} p99`, colour },
            { label: `${address} p50`, colour, dash: true },
          ];
        }),
        format: (value) => (value === 0 ? null : msAxisLabel(value)),
      }),
    [theme, addresses],
  );

  const data = useMemo<uPlot.AlignedData>(() => {
    const rows = history?.items ?? [];
    return [
      rows.map((item) => epochSeconds(item.ts)),
      // An exact `0.0` is an interval in which the endpoint answered nothing,
      // not a round trip of zero. `spanGaps` is off, so the line breaks there
      // instead of being bridged into a dip that never happened.
      ...addresses.flatMap((address) => [
        rows.map((item) => rttMs(item, address, 'p99')),
        rows.map((item) => rttMs(item, address, 'p50')),
      ]),
    ] as uPlot.AlignedData;
  }, [history, addresses]);

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
    <Card title="Round-trip time by endpoint, p50 and p99" tools={chips}>
      <div class="chips-mobile">{chips}</div>
      {body()}
      <div class="chart-legend latency-legend">
        {addresses.map((address, index) => (
          <span key={address}>
            <span class="sw" style={{ background: colourFor(theme, index) }} />
            <span class="mono">{address}</span>
          </span>
        ))}
        <span class="note">solid p99 · dashed p50</span>
      </div>
      <p class="chart-footnote">
        Network time, measured over the attempts that were answered — a timeout
        is counted as a failure beside it, not as a slow answer. This is the
        part of the Performance page's <b>forward</b> stage that is not
        FastAdHunter.
        <span class="footnote-line">
          Percentiles are bucket-granularity estimates over each sampling
          interval and saturate at the top finite bucket, 2 s. Good for a trend
          line, not exact quantiles.
        </span>
      </p>
    </Card>
  );

  function body() {
    if (error !== null) return <ErrorState error={error} />;
    const empty = addresses.length === 0;
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

/**
 * The endpoints to draw, in the configured order the rows carry. Taken from
 * the **last** row rather than the first: a config reload mid-range changes the
 * set, and the newest row is the one whose endpoints still exist.
 */
export function endpointOrder(items: readonly PerfItem[]): string[] {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const upstreams = items[index]?.upstreams;
    if (upstreams !== undefined && upstreams.length > 0) {
      return upstreams.slice(0, MAX_SERIES).map((upstream) => upstream.address);
    }
  }
  return [];
}

function rttMs(
  item: PerfItem,
  address: string,
  percentile: 'p50' | 'p99',
): number | null {
  const upstream = item.upstreams?.find((row) => row.address === address);
  const seconds = upstream?.rtt?.[percentile];
  if (seconds === undefined || seconds === 0) return null;
  return seconds * 1000;
}
