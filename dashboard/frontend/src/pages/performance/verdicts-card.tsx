import { useMemo } from 'preact/hooks';
import type uPlot from 'uplot';
import type { HistoryPerf } from '../../api/types';
import { compactCount, epochSeconds } from '../../charts/format';
import { lineChartOptions } from '../../charts/lines';
import { useChartTheme } from '../../charts/theme';
import { Card } from '../../components/card';
import { Chart } from '../../components/chart';
import { EmptyState } from '../../components/empty-state';
import { ErrorState } from '../../components/error-state';
import { passDelta } from '../../derive';
import { AxisEnds } from './axis-ends';

/**
 * The three verdicts, per interval.
 *
 * **`allow` here is the real allow verdict** — `allowed_delta`, an explicit
 * exception match the engine counted — and not the derived `permitted` band the
 * Dashboard chart draws. The two are different figures and never share a word.
 * `pass` is the one derived series (E16), and it is the derivation
 * `crates/fah-model/src/perf.rs` documents.
 */
export function VerdictsCard({
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
        // Drawn back to front by magnitude, each from the same zero baseline —
        // the bands overlap rather than stack, so nothing is summed and no
        // band can disagree with the counter behind it.
        series: [
          { label: 'pass', colour: theme.series4, area: true },
          { label: 'block', colour: theme.blocked, area: true },
          { label: 'allow', colour: theme.series2, area: true },
        ],
        format: (value) => (value === 0 ? null : compactCount(value)),
      }),
    [theme],
  );

  const data = useMemo<uPlot.AlignedData>(() => {
    const rows = history?.items ?? [];
    return [
      rows.map((item) => epochSeconds(item.ts)),
      rows.map((item) =>
        item.queries_delta === undefined
          ? null
          : passDelta(
              item.queries_delta,
              item.blocked_delta ?? 0,
              item.allowed_delta ?? 0,
            ),
      ),
      rows.map((item) => item.blocked_delta ?? null),
      rows.map((item) => item.allowed_delta ?? null),
    ] as uPlot.AlignedData;
  }, [history]);

  return (
    <Card title="Verdicts per interval" secondary="pass · allow · block">
      {body()}
      <div class="chart-legend verdicts-legend">
        <span>
          <span class="sw" style={{ background: 'var(--series-4)' }} />
          pass
        </span>
        <span>
          <span class="sw" style={{ background: 'var(--series-blocked)' }} />
          block
        </span>
        <span>
          <span class="sw" style={{ background: 'var(--series-2)' }} />
          allow
        </span>
      </div>
      <p class="chart-footnote">
        <span class="mono">allow</span> is the explicit exception verdict — a
        rule that overrode a block — so it typically sits orders of magnitude
        below <span class="mono">pass</span>. It is not the{' '}
        <span class="mono">permitted</span> band the Dashboard draws, which is
        every query that was not blocked.
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
        height={140}
        footer={<AxisEnds items={history?.items ?? []} />}
        {...(stride === undefined ? {} : { decimatedBy: stride })}
      />
    );
  }
}
