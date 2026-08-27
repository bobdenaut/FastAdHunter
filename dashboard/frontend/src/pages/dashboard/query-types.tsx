import { useMemo } from 'preact/hooks';
import type { HistorySummary } from '../../api/types';
import { percent1 } from '../../charts/format';
import { Card } from '../../components/card';
import { Donut } from '../../components/donut';
import { EmptyState } from '../../components/empty-state';
import { queryTypeSlices, sumPerType } from '../../derive';
import { RANGES, type RangeKey } from './ranges';

/**
 * R13 / R14. The donut reads the **same response as the chart above it**, so
 * one fetch feeds both cards and `history.enabled = false` correctly disables
 * both. That is also why the card states the active range rather than a fixed
 * "last 24 h": the range selector governs it.
 */
export function QueryTypes({
  range,
  summary,
  recording,
  className,
}: {
  range: RangeKey;
  summary: HistorySummary | null;
  recording: boolean;
  className?: string;
}) {
  const slices = useMemo(
    () => queryTypeSlices(sumPerType(summary?.items ?? [])),
    [summary],
  );
  const total = slices.reduce((sum, slice) => sum + slice.value, 0);

  return (
    <Card
      title="Query types"
      secondary={`last ${RANGES[range].label}`}
      bodyClass="donut-body"
      className={className}
    >
      {!recording || slices.length === 0 ? (
        <EmptyState
          title={recording ? 'No data in this range' : 'History is not being recorded'}
        />
      ) : (
        <>
          <Donut
            segments={slices.map((slice, index) => ({
              label: slice.label,
              value: slice.value,
              colour: `var(--series-${String(Math.min(index + 1, 5))})`,
            }))}
            label={`Query types over the last ${RANGES[range].label}`}
            size={150}
            thickness={22}
          />
          <table class="donut-legend">
            <tbody>
              {slices.map((slice, index) => (
                <tr key={slice.label}>
                  <td>
                    <span
                      class="sw"
                      style={{
                        background: `var(--series-${String(Math.min(index + 1, 5))})`,
                      }}
                    />
                    {slice.label}
                  </td>
                  <td class="num">{slice.value.toLocaleString()}</td>
                  <td class="num share">
                    {percent1((slice.value / total) * 100)}%
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </Card>
  );
}
