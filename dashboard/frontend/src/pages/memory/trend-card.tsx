import { useMemo, useState } from 'preact/hooks';
import type uPlot from 'uplot';
import type { HistoryPerf, PerfItem } from '../../api/types';
import { epochSeconds, formatMiB } from '../../charts/format';
import { useChartTheme } from '../../charts/theme';
import { Card } from '../../components/card';
import { Chart } from '../../components/chart';
import { EmptyState } from '../../components/empty-state';
import { ErrorState } from '../../components/error-state';
import { stackedMemory, statsBytes } from '../../derive';
import { RANGE_KEYS, RANGES, type RangeKey } from '../dashboard/ranges';
import { WATCH_THRESHOLD } from './budgets';
import { memoryTrendOptions } from './trend-options';

/**
 * Where RSS goes, over time.
 *
 * The bands are **cumulative** sums, bottom-up, so the areas stack, and the top
 * edge is `rss_bytes` itself by the server-side identity rather than a
 * re-derivation. Residual is therefore the gap between `accounted` and the
 * total, drawn as the top band, so a leak reads as one thing: **the top
 * thickening while the others stay flat.**
 *
 * **`peak_rss` is its own dashed line beside the stack, never derived from it.**
 * The compile peak lasts seconds against a sampling interval of minutes, so the
 * sampled series has never contained it — which is the entire reason a
 * high-water mark is charted separately. A `0` row is a gap, not a floor: it
 * means the row predates the field or `getrusage` was unavailable.
 */
export function TrendCard({
  history,
  error,
  loading,
  range,
  onRange,
}: {
  history: HistoryPerf | null;
  error: Error | null;
  loading: boolean;
  range: RangeKey;
  onRange: (next: RangeKey) => void;
}) {
  const theme = useChartTheme();
  const [cursor, setCursor] = useState<number | null>(null);
  const [place, setPlace] = useState<{ x: number; y: number } | null>(null);
  const items = history?.items ?? [];

  const options = useMemo(
    () =>
      memoryTrendOptions({
        theme,
        range,
        onCursor: setCursor,
        onPlace: setPlace,
      }),
    [theme, range],
  );

  const data = useMemo<uPlot.AlignedData>(() => {
    const xs = items.map((item) => epochSeconds(item.ts));
    const bands = stackedMemory(items);
    const peak = items.map((item) =>
      item.peak_rss === undefined || item.peak_rss === 0 ? null : item.peak_rss,
    );
    const faults = items.map((item) =>
      item.minor_page_faults === undefined || item.minor_page_faults === 0
        ? null
        : item.minor_page_faults,
    );
    return [xs, ...bands, peak, faults] as unknown as uPlot.AlignedData;
  }, [items]);

  const hovered = cursor === null ? null : (items[cursor] ?? null);

  return (
    <Card
      title="Memory usage over time"
      className="memory-trend"
      tools={
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
      }
    >
      <Legend />

      {error !== null ? (
        <ErrorState error={error} />
      ) : items.length === 0 ? (
        <EmptyState title={loading ? 'Loading…' : 'No samples in this window'}>
          The perf series is persisted to <span class="mono">/data/history</span>{' '}
          and is bounded by <span class="mono">history.retention_days</span>. An
          empty window is a normal answer, not a failure.
        </EmptyState>
      ) : (
        <div class="memory-plot">
          <Chart
            data={data}
            options={options}
            height={300}
            {...(history?.stride === undefined ? {} : { decimatedBy: history.stride })}
          />
          {hovered !== null && place !== null && (
            <Tip
              item={hovered}
              stride={history?.stride ?? 1}
              at={place}
            />
          )}
        </div>
      )}

      <p class="note">
        The stack sums to RSS and residual is the top band, so a leak reads as
        one thing: <b>the top thickening while the others stay flat</b>.
        <span class="footnote-line">
          The compile peak does not appear in the area, because it lasts seconds
          against the sampling interval — which is why peak is charted beside the
          stack rather than derived from it.
        </span>
        <span class="footnote-line">
          <b>RSS is the stack's top edge, in ink</b> — in a stack that sums to
          RSS, that line <i>is</i> RSS, so it needs no colour of its own. It
          turns red only above the steady-state budget, and the band below that
          starts at {formatMiB(WATCH_THRESHOLD)}: above anything the sampled
          series has recorded, and still under budget.
        </span>
        <span class="footnote-line">
          Cache and stats are hairlines at this scale. That is the true
          proportion, not a rendering fault — their exact figures are in the
          composition card and in the readout above.
        </span>
        <span class="footnote-line">
          <b>A dashed rule marks a restart</b>, with the process's new peak on
          it. It is drawn where peak RSS or the minor-fault counter falls: both
          only ever grow over a process lifetime, so a fall in either is a
          restart and never a reclaim. The peak alone is not enough, because
          every process's peak is its startup compile and consecutive processes
          peak alike — and that compile lasts seconds against the sampling
          interval, so it never appears in the area below.
        </span>
      </p>
    </Card>
  );
}

/** Identity never rests on colour alone: every entry carries its label, and the
 *  two that are not hues say what they are. */
function Legend() {
  return (
    <div class="memory-legend">
      <span>
        <span class="sw-line" /> RSS <span class="note">— the stack top</span>
      </span>
      <span>
        <span class="sw sw-residual" /> residual / unaccounted
      </span>
      <span>
        <span class="sw sw-ruleset" /> ruleset
      </span>
      <span>
        <span class="sw sw-cache" /> cache
      </span>
      <span>
        <span class="sw sw-stats" /> stats
      </span>
      <span>
        <span class="sw-dash" /> peak RSS
      </span>
      <span>
        <span class="sw sw-watch" /> watch
      </span>
      <span>
        <span class="sw sw-over" /> over budget
      </span>
    </div>
  );
}

/**
 * The hovered sample, in a box over the plot — the artboard's treatment.
 *
 * **It is DOM, not canvas.** Canvas text cannot be selected or reached by a
 * screen reader, and laying out two aligned columns on canvas means
 * re-implementing font metrics. The plugin supplies the placement, because only
 * it knows the plot's box; this owns the markup, because only it knows the
 * labels.
 *
 * It is also the only place cache and stats are readable at all — they are
 * hairlines in the stack by true proportion, and their figures live here and in
 * the composition card.
 *
 * `aria-hidden`: it is a pointer affordance duplicating figures the cards
 * already state, so announcing it on every cursor move would be noise.
 */
function Tip({
  item,
  stride,
  at,
}: {
  item: PerfItem;
  stride: number;
  at: { x: number; y: number };
}) {
  const memory = item.memory;
  // One anchor. The plugin has already flipped it past the crosshair and
  // clamped it inside the card, because that is where the plot's geometry is.
  const style = `left: ${String(Math.round(at.x))}px; top: ${String(at.y)}px`;
  return (
    <div class="memory-tip" style={style} aria-hidden="true">
      <div class="mono memory-tip-ts">
        {new Date(item.ts).toLocaleTimeString()}
        {stride > 1 ? ` · every ${String(stride)}th sample` : ' · every sample'}
      </div>
      <Row tone="ink" label="RSS" bytes={item.rss_bytes} />
      <Row tone="residual" label="residual" bytes={memory?.residual_bytes} />
      <Row tone="ruleset" label="ruleset" bytes={memory?.ruleset_bytes} />
      <Row tone="cache" label="cache" bytes={memory?.cache_estimated_bytes} />
      <Row
        tone="stats"
        label="stats"
        bytes={memory === undefined ? undefined : statsBytes(memory)}
      />
      {/* Peak sits below a rule and in muted ink: it is a lifetime high-water
          mark, not a member of the stack the five rows above sum to. */}
      <div class="memory-tip-split" />
      <Row
        tone="peak"
        label="peak"
        dash
        muted
        bytes={item.peak_rss === 0 ? undefined : item.peak_rss}
      />
    </div>
  );
}

function Row({
  tone,
  label,
  bytes,
  dash,
  muted,
}: {
  tone: string;
  label: string;
  bytes: number | undefined;
  dash?: boolean;
  muted?: boolean;
}) {
  return (
    <div class={muted === true ? 'memory-tip-row muted' : 'memory-tip-row'}>
      <span class={dash === true ? 'sw-dash' : `sw sw-${tone}`} />
      <span class="memory-tip-label">{label}</span>
      <span class="num">{bytes === undefined ? '—' : formatMiB(bytes)}</span>
    </div>
  );
}
