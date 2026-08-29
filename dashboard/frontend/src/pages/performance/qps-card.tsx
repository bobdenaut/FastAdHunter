import { useMemo } from 'preact/hooks';
import type uPlot from 'uplot';
import type { HistoryPerf } from '../../api/types';
import { compactCount, epochSeconds, qpsLabel } from '../../charts/format';
import { lineChartOptions } from '../../charts/lines';
import { useChartTheme } from '../../charts/theme';
import { Card } from '../../components/card';
import { Chart } from '../../components/chart';
import { EmptyState } from '../../components/empty-state';
import { ErrorState } from '../../components/error-state';
import { qpsStats } from '../../derive';
import { AxisEnds } from './axis-ends';
import { SUSTAINED_QPS, SUSTAINED_QPS_NOTE } from './budgets';

/**
 * `qps` is per-interval, not cumulative — it is `queries_delta` over the
 * sampling interval, computed by the recorder rather than here.
 *
 * The stat row is scoped to the **served** rows (E14). At `stride > 1` the
 * response is a one-in-stride subsample, so the busiest row served is not
 * necessarily the range's peak and the last one is not now; the labels say
 * exactly that rather than implying a live reading.
 */
export function QpsCard({
  history,
  error,
  loading,
}: {
  history: HistoryPerf | null;
  error: Error | null;
  loading: boolean;
}) {
  const theme = useChartTheme();

  const options = useMemo(
    () =>
      lineChartOptions({
        theme,
        series: [{ label: 'qps', colour: theme.series1, area: true }],
        format: (value) => (value === 0 ? null : `${compactCount(value)} /s`),
      }),
    [theme],
  );

  const data = useMemo<uPlot.AlignedData>(() => {
    const rows = history?.items ?? [];
    return [
      rows.map((item) => epochSeconds(item.ts)),
      rows.map((item) => item.qps ?? null),
    ] as uPlot.AlignedData;
  }, [history]);

  // The previous range's rows stay in state so the plot survives a retry, but
  // this row is scoped to the *selected* range — so a failed read empties it
  // rather than attributing another window's figures to this one.
  const stats = useMemo(
    () =>
      error === null
        ? qpsStats((history?.items ?? []).map((item) => item.qps))
        : { latest: null, busiest: null },
    [history, error],
  );

  return (
    <Card title="Queries per second" secondary="per-interval, not cumulative">
      {body()}
      <div class="stat-row">
        <Stat
          value={stats.latest === null ? '—' : qpsLabel(stats.latest)}
          label="latest sample"
        />
        <Stat
          value={stats.busiest === null ? '—' : qpsLabel(stats.busiest)}
          label="busiest served sample"
        />
        <Stat value={SUSTAINED_QPS} label={SUSTAINED_QPS_NOTE} />
      </div>
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
        height={140}
        footer={<AxisEnds items={history?.items ?? []} />}
        {...(stride === undefined ? {} : { decimatedBy: stride })}
      />
    );
  }
}

function Stat({ value, label }: { value: string; label: string }) {
  return (
    <div>
      <div class="stat-figure mono">{value}</div>
      <div class="note">{label}</div>
    </div>
  );
}
