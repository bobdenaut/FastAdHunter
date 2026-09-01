import { useMemo, useState } from 'preact/hooks';
import type { HistorySummary } from '../../api/types';
import { percent1 } from '../../charts/format';
import { Card } from '../../components/card';
import { Donut } from '../../components/donut';
import { EmptyState } from '../../components/empty-state';
import {
  REST_LABEL,
  foldedLabels,
  queryTypeSlices,
  sliceShare,
  sumPerType,
} from '../../derive';
import { RANGES, plottedRange, type RangeKey } from './ranges';

/**
 * R13 / R14. The donut reads the **same response as the chart above it**, so
 * one fetch feeds both cards and `history.enabled = false` correctly disables
 * both. That is also why the card states the active range rather than a fixed
 * "last 24 h": the range selector governs it.
 *
 * **Its hover is a highlight, not a tooltip**, and that is the one place this
 * card deliberately differs from the chart above it. A bar carries no figures
 * of its own, so the chart has to raise a tooltip to answer "how many"; the
 * ring is drawn beside a legend that already prints every count and share, so
 * a tooltip here would cover the very numbers it repeats. Pointing at a slice
 * marks its row instead — one fact, in one place, made findable.
 */
export function QueryTypes({
  range,
  summary,
  recording,
  loading,
  className,
}: {
  range: RangeKey;
  summary: HistorySummary | null;
  recording: boolean;
  /** Held across the `/config` re-read the chart above triggers, for the same
   *  reason: this card must not name the wrong empty state either. */
  loading: boolean;
  className?: string;
}) {
  const totals = useMemo(() => sumPerType(summary?.items ?? []), [summary]);
  const slices = useMemo(() => queryTypeSlices(totals), [totals]);
  const total = slices.reduce((sum, slice) => sum + slice.value, 0);

  /** Both words the legend can print that are not a record type the client
   *  asked for. `OTHER` is the API's own catch-all and is glossed only when it
   *  is drawn as its own row — folded into `rest`, it is one of the names the
   *  line above already lists. */
  const folded = useMemo(() => foldedLabels(totals, slices), [totals, slices]);
  const showsOther = slices.some((slice) => slice.label === 'OTHER');

  // The range of what is **on screen**, not of what was last asked for. The
  // slices lag a range change by the round trip, so naming the chip's range
  // would print `last 24 h` over the 30 d figures until the response landed.
  // Only the response moves this, so the words and the numbers always describe
  // the same window — the chart's `plottedResolution` rule, applied to a label.
  const plotted = plottedRange(summary, range);
  const rangeLabel = RANGES[plotted].label;

  /** The slice the pointer is on, shared by the ring and the legend so either
   *  one can raise it and both show it. */
  const [hovered, setHovered] = useState<number | null>(null);

  const tone = (index: number) =>
    `var(--series-${String(Math.min(index + 1, 5))})`;

  return (
    <Card
      title="Query types"
      secondary={`last ${rangeLabel}`}
      bodyClass="donut-body"
      className={className}
    >
      {recording && loading && slices.length === 0 ? (
        <div class="boot" />
      ) : !recording || slices.length === 0 ? (
        <EmptyState
          title={recording ? 'No data in this range' : 'History is not being recorded'}
        />
      ) : (
        <>
          <div class="donut-main">
            <Donut
              segments={slices.map((slice, index) => ({
                label: slice.label,
                value: slice.value,
                colour: tone(index),
              }))}
              label={`Query types over the last ${rangeLabel}`}
              size={150}
              thickness={22}
              hovered={hovered}
              onHover={setHovered}
            />
            <table class="donut-legend">
              <tbody>
                {slices.map((slice, index) => (
                  <tr
                    key={slice.label}
                    class={hovered === index ? 'on' : undefined}
                    // The row's accent bar is the slice's own colour, so the
                    // mark says *which* arc it belongs to and not merely that
                    // something is marked.
                    style={{ '--slice': tone(index) }}
                    // The legend answers the pointer as well as the ring, and
                    // raises the same highlight: on a phone it is the larger
                    // target by a wide margin, and it is the one a thumb can
                    // hit without landing on a 22 px band.
                    onPointerEnter={() => setHovered(index)}
                    onPointerLeave={() => setHovered(null)}
                  >
                    <td>
                      <span class="sw" style={{ background: tone(index) }} />
                      {slice.label}
                    </td>
                    <td class="num">{slice.value.toLocaleString()}</td>
                    <td class="num share">
                      {percent1(sliceShare(slice.value, total))}%
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {(folded.length > 0 || showsOther) && (
            <p class="donut-note">
              {folded.length > 0 && (
                <span>
                  {REST_LABEL} — {folded.join(', ')}.
                </span>
              )}
              {showsOther && (
                <span>OTHER — record types outside the tracked ten.</span>
              )}
            </p>
          )}
        </>
      )}
    </Card>
  );
}
