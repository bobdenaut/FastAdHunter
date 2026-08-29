import type uPlot from 'uplot';
import { loadedUPlot } from './runtime';
import { niceMax, withAlpha, ySplits } from './scale';
import type { ChartTheme } from './theme';

/**
 * Multi-series lines and filled areas — the Performance page's three charts,
 * and the second (and last) chart family in the phase.
 *
 * **No uPlot symbol is imported at module scope**, types excepted; the device
 * pixel ratio comes from `charts/runtime.ts`, which owns the one dynamic
 * import.
 *
 * Two properties this family exists for and the bar chart has no use for:
 *
 * - **Gaps are gaps.** `spanGaps` is off on every series, so a `null` is drawn
 *   as a break rather than bridged. A latency sample of exactly `0.0` means no
 *   traffic in that stage in that interval, and bridging it would draw a dip
 *   the engine never had.
 * - **A budget is a marker, never a wall.** It is drawn by a hook as a dashed
 *   rule with its own label, never as a data series — a series would join the
 *   legend, enter the y range as data and read as something the engine
 *   enforces. Nothing enforces it at runtime.
 */

/** What a series contributes to the y maximum before it is rounded up. */
const AREA_ALPHA = 0.22;

export interface LineSeriesSpec {
  label: string;
  colour: string;
  /** p50 in a p50/p99 pair. The artboard draws the lower percentile dashed and
   *  lighter, so the pair reads as one stage rather than two. */
  dash?: boolean;
  /** Fills to the zero baseline, for the QPS and verdict areas. */
  area?: boolean;
}

/** A dashed target line with its own right-aligned caption, in the plot. */
export interface BudgetMarker {
  value: number;
  label: string;
}

export interface LinesInput {
  theme: ChartTheme;
  series: readonly LineSeriesSpec[];
  /** The y-axis tick text. `null` for a split that should print nothing — the
   *  artboards leave the zero line unlabelled. */
  format: (value: number) => string | null;
  budget?: BudgetMarker;
}

const DASH_PATTERN = [4, 3];
const DASH_ALPHA = 0.55;
const LINE_WIDTH = 1.8;
const DASH_WIDTH = 1.4;
const BUDGET_WIDTH = 1.2;
const BUDGET_DASH = [5, 4];
const BUDGET_LABEL_PX = 10;
const BUDGET_LABEL_GAP_PX = 3;

/** uPlot scales its canvas by a module-level ratio, not a per-instance one, so
 *  this is where the device pixels a hook works in are converted back. */
function pixelRatio(): number {
  return loadedUPlot()?.pxRatio ?? 1;
}

/**
 * The budget rule, drawn after the series so a marker on a busy plot stays
 * findable. It is one hook and it reads nothing but the y scale, so it costs
 * the same whatever the range holds.
 */
export function budgetPlugin(
  theme: ChartTheme,
  budget: BudgetMarker,
): uPlot.Plugin {
  return {
    hooks: {
      draw: (u: uPlot) => {
        const ratio = pixelRatio();
        const y = u.valToPos(budget.value, 'y', true);
        if (!Number.isFinite(y)) return;
        const ctx = u.ctx;
        ctx.save();
        ctx.beginPath();
        ctx.strokeStyle = theme.budget;
        ctx.lineWidth = BUDGET_WIDTH * ratio;
        ctx.setLineDash(BUDGET_DASH.map((step) => step * ratio));
        ctx.moveTo(u.bbox.left, y);
        ctx.lineTo(u.bbox.left + u.bbox.width, y);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.font = `${String(BUDGET_LABEL_PX * ratio)}px ${theme.mono}`;
        ctx.fillStyle = theme.tick;
        ctx.textAlign = 'right';
        ctx.textBaseline = 'bottom';
        ctx.fillText(
          budget.label,
          u.bbox.left + u.bbox.width,
          y - BUDGET_LABEL_GAP_PX * ratio,
        );
        ctx.restore();
      },
    },
  };
}

/**
 * **Must be memoised on `(range, theme)`** — the wrapper keys the uPlot
 * instance on this object's identity, so a fresh literal per render would
 * destroy and rebuild the plot on every render (p5-05's finding m4).
 *
 * The y range always contains the budget: a stage comfortably inside its target
 * would otherwise scale the plot to itself and put the marker off the top,
 * which is the one thing the marker is there to be seen against.
 */
export function lineChartOptions(
  input: LinesInput,
): Omit<uPlot.Options, 'width' | 'height'> {
  const { theme, series, format, budget } = input;

  return {
    // The legend is our own markup below the plot, as the artboards draw it.
    legend: { show: false },
    cursor: {
      // These plots carry no tooltip: the figures a reader needs are the tiles
      // and the stat row beside them, which are readable without a pointer.
      x: false,
      y: false,
      drag: { x: false, y: false },
      points: { show: false },
    },
    scales: {
      x: { time: true },
      y: {
        range: (_u, _min, max) =>
          [0, niceMax(Math.max(max, budget?.value ?? 0))] as uPlot.Range.MinMax,
      },
    },
    axes: [
      {
        // The artboards draw no x ticks on these plots: the span is named in
        // words beneath the axis instead, because a per-sample series over a
        // month has no tick density that is both honest and readable.
        show: false,
      },
      {
        // Right-hand side, labels outside the plot — the artboards' treatment.
        side: 1,
        stroke: theme.tick,
        grid: { stroke: theme.grid, width: 1 },
        ticks: { show: false },
        font: `10px ${theme.mono}`,
        size: 46,
        splits: (u) => ySplits(u.scales['y']?.max ?? 0),
        values: (_u, splits) => splits.map((value) => format(value)),
      },
    ],
    series: [
      {},
      ...series.map((spec) => ({
        label: spec.label,
        stroke: spec.dash === true ? withAlpha(spec.colour, DASH_ALPHA) : spec.colour,
        width: spec.dash === true ? DASH_WIDTH : LINE_WIDTH,
        ...(spec.dash === true ? { dash: DASH_PATTERN } : {}),
        ...(spec.area === true
          ? { fill: withAlpha(spec.colour, AREA_ALPHA) }
          : {}),
        // A `null` is a break in the line, never a bridge — see the header.
        spanGaps: false,
        points: { show: false },
      })),
    ],
    ...(budget === undefined
      ? {}
      : { plugins: [budgetPlugin(theme, budget)] }),
  };
}
