import type { PerfItem } from '../../api/types';
import { formatAge } from '../../time';

/**
 * E19 — the window's two ends, beneath the plot, where the artboards draw them
 * in place of x ticks. A per-sample series over a month has no tick density
 * that is both honest and readable, so the span is named in words instead.
 *
 * Both ends are a formatting of `items[].ts`, and the right-hand one is the age
 * of the **latest served row** rather than the word `now`: decimation drops
 * whole rows, so at a wide range the last point plotted can be an interval or
 * more behind real time, and calling it `now` would be the one claim this page
 * takes care not to make.
 */
export function AxisEnds({ items }: { items: readonly PerfItem[] }) {
  const first = items[0];
  const last = items[items.length - 1];
  if (first === undefined || last === undefined) return null;
  const now = Date.now();
  return (
    <div class="axis-ends mono">
      <span>{formatAge(Date.parse(first.ts), now)}</span>
      <span>{formatAge(Date.parse(last.ts), now)}</span>
    </div>
  );
}
