import { useMemo } from 'preact/hooks';
import type uPlot from 'uplot';
import type { HistoryPerf } from '../../api/types';
import { epochSeconds } from '../../charts/format';
import { lineChartOptions } from '../../charts/lines';
import { useChartTheme } from '../../charts/theme';
import { Card } from '../../components/card';
import { Chart } from '../../components/chart';
import { EmptyState } from '../../components/empty-state';
import { faultRate, windowTrend } from '../../derive';
import { RANGES, type RangeKey } from '../dashboard/ranges';

/**
 * Minor page faults, as a **rate**.
 *
 * `minor_page_faults` is cumulative since process start, so charting it draws a
 * ramp and says nothing. Its derivative is the purge-thrash detector: a rising
 * rate at flat RSS means the purge delay is returning pages the allocator is
 * about to fault straight back in, which is exactly the trade
 * `MIMALLOC_PURGE_DELAY` tunes and is otherwise invisible — RSS looks healthy
 * while the process pays a syscall and a fault on memory it is about to reuse.
 *
 * **A restart resets the counter**, so a negative difference is a new process
 * rather than a negative rate. Those samples are dropped rather than clamped to
 * zero: a zero would read as a quiet minute that never happened.
 *
 * The line wears ink, not a hue. It is one series in its own card with nothing
 * beside it to be confused with, and every categorical hue on this page is
 * already spoken for by the stack.
 */
export function FaultsCard({
  history,
  range,
}: {
  history: HistoryPerf | null;
  /** Named on the card, because the page-level chips change what it shows. */
  range: RangeKey;
}) {
  const theme = useChartTheme();
  const items = history?.items ?? [];

  // `faultRate` (D12) is the shared derivation and already answers `null` for a
  // restart rather than a negative rate. This walks the window with it; the
  // rule about what a rate means lives in one place.
  const rates = useMemo(() => {
    const xs: number[] = [];
    const ys: Array<number | null> = [];
    for (let index = 1; index < items.length; index += 1) {
      const previous = items[index - 1];
      const current = items[index];
      if (previous === undefined || current === undefined) continue;
      xs.push(epochSeconds(current.ts));
      ys.push(faultRate(previous, current));
    }
    return { xs, ys };
  }, [items]);

  const options = useMemo(
    () =>
      lineChartOptions({
        theme,
        series: [{ label: 'minor faults /s', colour: theme.ink }],
        format: (value: number) =>
          value === 0 ? null : `${value.toFixed(0)} /s`,
      }),
    [theme],
  );

  const data = useMemo<uPlot.AlignedData>(
    () => [rates.xs, rates.ys] as unknown as uPlot.AlignedData,
    [rates],
  );

  const latest = [...rates.ys].reverse().find((value) => value !== null) ?? null;

  // The artboard prints `118 /s · flat` — the descriptor is the reading that
  // matters here, since a *rising* rate at flat RSS is the purge-thrash
  // signature. It is measured, not decorative: `windowTrend` (D14) answers
  // `null` on a window too short to have a shape, and then no word is printed
  // rather than a steadiness nobody observed. 20 % because a fault rate is far
  // noisier sample-to-sample than residual is.
  const trend = windowTrend(
    rates.ys.filter((value): value is number => value !== null),
    0.2,
  );

  return (
    <Card
      title="Minor page faults"
      secondary={`the rate over ${RANGES[range].label}, never the counter`}
    >
      {latest === null ? (
        <EmptyState title="No rate yet">
          A rate needs two consecutive samples. The window holds fewer, or the
          only pair it holds spans a restart.
        </EmptyState>
      ) : (
        <>
          <div class="faults-figure">
            <span class="num faults-value">{latest.toFixed(0)}</span>
            <span class="note">
              &nbsp;/s{trend === null ? '' : ` · ${trend}`}
            </span>
          </div>
          {/* One interval is a rate but not a line. The figure above is the
              reading either way; the plot appears once there is a shape to
              draw, rather than a single point pretending to be a trend. */}
          {rates.xs.length >= 2 && (
            <>
              <Chart data={data} options={options} height={150} />
              {/* The shared line chart draws no x ticks, so the window's two
                  ends are labelled here — without them the plot is a shape with
                  no stated span, which is what the artboard's `24 h ago` / `now`
                  pair exists to prevent. */}
              <div class="faults-xends note">
                <span>{RANGES[range].label} ago</span>
                <span>now</span>
              </div>
            </>
          )}
        </>
      )}
      <p class="note">
        The counter is cumulative since process start, so charting it draws a
        ramp and says nothing. Its derivative is the purge-thrash detector:{' '}
        <b>a rising rate at flat RSS</b> means the purge delay is returning pages
        the allocator is about to fault straight back in. A restart resets the
        counter, so those samples are dropped rather than drawn as a fall.
      </p>
    </Card>
  );
}
