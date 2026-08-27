import type uPlot from 'uplot';
import type { HistoryResolution } from '../api/types';
import { axisTimeLabel, bucketWindowLabel, compactCount, percent1 } from './format';
import { loadedUPlot } from './runtime';
import type { ChartTheme } from './theme';

/**
 * The permitted/blocked bar chart, and the only chart engineering in the phase
 * — `p5-08` reuses it rather than inventing a second one.
 *
 * **No uPlot symbol is imported at module scope**, types excepted: the bar path
 * builder comes from `charts/runtime.ts`, which owns the one dynamic import.
 */

/**
 * **Stacking without stacking arithmetic.** uPlot has no stacked-bar mode. Both
 * series are drawn from zero and painted back to front: `queries` first in the
 * permitted colour, `blocked` second on top of it. The visible upper band is
 * `queries − blocked` **by occlusion**, so no sums are computed, the y maximum
 * is `max(queries)` with no accumulation error, and the band cannot disagree
 * with the total above it.
 *
 * `permitted` is the word; `allow` is a different figure and never appears.
 */
export const QUERIES_SERIES = 1;
export const BLOCKED_SERIES = 2;

/** The artboard's bar in its slot: a tenth of the pitch is the gap. */
const BAR_GAP_RATIO = 0.9;
const MAX_BAR_PX = 60;

/** What a non-hovered bar keeps of its colour. */
const DIM_ALPHA = 0.35;

/* --------------------------------------------------- the printed-figure floors */

/**
 * A bar at least this wide carries its total above it; a blocked segment at
 * least this tall carries its own figure inside it. Below either floor nothing
 * is drawn — **dropped, never shrunk**. That single rule is what makes the
 * phone work with no phone-specific code and what stops 30 daily bars becoming
 * a wall of digits, and it is why neither artboard needed a second layout to
 * state its own labelling.
 *
 * Both are CSS pixels: every measurement below is divided by the device pixel
 * ratio first, so the floors mean the same thing on a phone and on a retina
 * desktop.
 */
export const TOTAL_LABEL_MIN_BAR_PX = 50;
export const SEGMENT_LABEL_MIN_PX = 15;

/** The width one bar is actually drawn at, gap ratio and cap included. */
export function barWidthPx(plotWidthPx: number, buckets: number): number {
  if (buckets <= 0 || plotWidthPx <= 0) return 0;
  return Math.min((plotWidthPx / buckets) * BAR_GAP_RATIO, MAX_BAR_PX);
}

export function showsTotalLabel(barPx: number): boolean {
  return barPx >= TOTAL_LABEL_MIN_BAR_PX;
}

/**
 * The blocked figure needs a segment tall enough **and** a bar wide enough:
 * `visual-system.md` says it is carried *inside* the segment, and a figure
 * wider than its own bar is not inside anything — at 20 px bars the labels of
 * neighbouring buckets touch. Both artboards agree: `Main.dc.html` prints the
 * total and the segment figure together at 51.8 px bars, and
 * `MobileDashboard.dc.html` prints neither at 11 px. Neither draws one without
 * the other.
 */
export function showsSegmentLabel(segmentPx: number, barPx: number): boolean {
  return segmentPx >= SEGMENT_LABEL_MIN_PX && showsTotalLabel(barPx);
}

/** uPlot scales its canvas by a module-level ratio, not a per-instance one, so
 *  this is where the device pixels the hooks work in are converted back. */
function pixelRatio(): number {
  return loadedUPlot()?.pxRatio ?? 1;
}

/* ------------------------------------------------------------------ the scale */

const NICE_STEPS = [1, 1.5, 2, 2.5, 3, 4, 5, 7.5, 10];

/**
 * A round upper bound so five evenly spaced gridlines land on figures a reader
 * can use: 13,169 becomes 15,000, which is what `Main.dc.html` draws.
 */
export function niceMax(max: number): number {
  if (!Number.isFinite(max) || max <= 0) return 4;
  const magnitude = 10 ** Math.floor(Math.log10(max));
  const ratio = max / magnitude;
  const step = NICE_STEPS.find((candidate) => ratio <= candidate) ?? 10;
  return step * magnitude;
}

/** Five gridlines, as the artboards draw. */
export function ySplits(max: number): number[] {
  const top = niceMax(max);
  return [0, top / 4, top / 2, (top * 3) / 4, top];
}

/* ------------------------------------------------------------------- hovering */

/**
 * Which bar the cursor is on, shared by the three parts that need it: the bar
 * fills, the printed labels and the tooltip. A mutable object rather than an
 * option, because the options must stay referentially stable — rebuilding them
 * per cursor move is exactly the plot-rebuild p5-05's finding m4 is about.
 */
export interface HoverState {
  index: number | null;
}

export function createHoverState(): HoverState {
  return { index: null };
}

/** What the tooltip prints. `blockedPercent` is the **served** per-item field:
 *  it is read, never divided. */
export interface HoverItem {
  ts: string;
  queries: number;
  blocked: number;
  blockedPercent: number;
}

/** `#rrggbb` to `rgba(...)`. A token that is not hex is returned unchanged
 *  rather than mangled — a dimmed bar is worth less than a drawn one. */
function withAlpha(colour: string, alpha: number): string {
  const hex = colour.trim();
  if (!/^#[0-9a-f]{6}$/i.test(hex)) return hex;
  const value = Number.parseInt(hex.slice(1), 16);
  const r = (value >> 16) & 0xff;
  const g = (value >> 8) & 0xff;
  const b = value & 0xff;
  return `rgba(${String(r)}, ${String(g)}, ${String(b)}, ${String(alpha)})`;
}

/* ------------------------------------------------------------------- options */

export interface StackedBarsInput {
  resolution: HistoryResolution;
  theme: ChartTheme;
  /** Shared with the label and hover plugins; see `HoverState`. */
  hover: HoverState;
  /** The label and hover plugins. Passed in so this factory stays a description
   *  of the plot and the plugins stay separately testable. */
  plugins?: uPlot.Plugin[];
}

/**
 * **Must be memoised on `(range, theme)`** — the wrapper keys the uPlot
 * instance on this object's identity, so a fresh literal per render would
 * destroy and rebuild the plot every time a `stats` push re-renders the page.
 * That is p5-05's finding m4, and its proof is a construction count taken in a
 * browser.
 */
export function stackedBarsOptions(
  input: StackedBarsInput,
): Omit<uPlot.Options, 'width' | 'height'> {
  const { resolution, theme, hover } = input;

  const fills = [theme.permitted, theme.blocked];
  const dimmed = fills.map((colour) => withAlpha(colour, DIM_ALPHA));

  // Per-bar fills, which is how the non-hovered bars dim: uPlot's own bars
  // renderer groups them by colour and builds one path per group. Built per
  // options object rather than once per module, so two charts on a page cannot
  // share one hover state.
  let barsBuilder: uPlot.Series.PathBuilder | null = null;

  const barsPath = (
    u: uPlot,
    seriesIdx: number,
    idx0: number,
    idx1: number,
  ): uPlot.Series.Paths | null => {
    if (barsBuilder === null) {
      // `paths.bars` is a static on the uPlot module, not on the instance —
      // uPlot returns a plain object, so `u.constructor` is `Object`.
      const runtime = loadedUPlot() as unknown as {
        paths: { bars: (config: unknown) => uPlot.Series.PathBuilder };
      } | null;
      if (runtime === null) return null;
      barsBuilder = runtime.paths.bars({
        size: [BAR_GAP_RATIO, MAX_BAR_PX],
        align: 0,
        disp: {
          fill: {
            unit: 3,
            values: (_u: uPlot, series: number, from: number, to: number) => {
              const full = fills[series - 1] ?? theme.permitted;
              const faded = dimmed[series - 1] ?? full;
              const colours: string[] = [];
              for (let index = from; index <= to; index += 1) {
                colours.push(
                  hover.index === null || hover.index === index ? full : faded,
                );
              }
              return colours;
            },
          },
        },
      });
    }
    return barsBuilder(u, seriesIdx, idx0, idx1);
  };

  return {
    // The legend is our own markup below the plot — the artboards draw it that
    // way, and it is why uPlot's legend styles are not reproduced.
    legend: { show: false },
    cursor: {
      x: true,
      y: false,
      // Pinch-zoom and drag-select are off: the range chips do that work, and
      // a gesture the artboard's own note calls undiscoverable is not a
      // substitute for a control. A tap raises a synthetic `mousemove`, so the
      // phone's "tap a bar" needs no second code path.
      drag: { x: false, y: false },
      points: { show: false },
    },
    scales: {
      x: { time: true },
      // Bars share a baseline of zero — that is what makes a ~12 % blocked
      // segment comparable bar to bar.
      y: { range: (_u, _min, max) => [0, niceMax(max)] },
    },
    axes: [
      {
        stroke: theme.tick,
        grid: { show: false },
        ticks: { show: false },
        font: `10px ${theme.mono}`,
        size: 26,
        // A fifth of the plot per label. uPlot's default is a flat 50 px, which
        // at 24 hourly bars over ~1200 px is one label per bar — a wall of
        // clock digits where `Main.dc.html` draws four. Asking for a fifth of
        // whatever width the plot has keeps that cadence at every width and at
        // every range, so the phone gets the same rule and no second branch.
        space: (_u, _axisIdx, _min, _max, dim) => dim / 5,
        values: (_u, splits) =>
          splits.map((value) => axisTimeLabel(value, resolution)),
      },
      {
        // Right-hand side, labels outside the plot: `Performance`'s treatment,
        // which the artboards draw for every series.
        side: 1,
        stroke: theme.tick,
        grid: { stroke: theme.grid, width: 1 },
        ticks: { show: false },
        font: `10px ${theme.mono}`,
        size: 46,
        splits: (u) => ySplits(u.scales['y']?.max ?? 0),
        values: (_u, splits) => splits.map((value) => compactCount(value)),
      },
    ],
    series: [
      {},
      // `width: 0` is not cosmetic: uPlot ignores per-bar fills unless the
      // series has no stroke. The artboards draw unoutlined bars anyway.
      {
        label: 'permitted',
        fill: theme.permitted,
        width: 0,
        paths: barsPath,
        points: { show: false },
      },
      {
        label: 'blocked',
        fill: theme.blocked,
        width: 0,
        paths: barsPath,
        points: { show: false },
      },
    ],
    ...(input.plugins === undefined ? {} : { plugins: input.plugins }),
  };
}

/* -------------------------------------------------------------- label plugin */

const TOTAL_LABEL_GAP_PX = 6;
const TOTAL_LABEL_SIZE_PX = 9.5;
const SEGMENT_LABEL_SIZE_PX = 9;

/**
 * uPlot prints nothing on bars, so this runs after the series are drawn and
 * puts the figures on. It reads the plotted data rather than the response, so
 * what it prints and what is drawn cannot disagree — and it dims with its bar,
 * because a crisp label over a faded bar reads as the hovered one.
 */
export function barLabelsPlugin(
  theme: ChartTheme,
  hover: HoverState,
): uPlot.Plugin {
  return {
    hooks: {
      draw: (u: uPlot) => {
        const xs = u.data[0];
        const queries = u.data[QUERIES_SERIES];
        const blocked = u.data[BLOCKED_SERIES];
        if (xs === undefined || queries === undefined || blocked === undefined) {
          return;
        }

        const ratio = pixelRatio();
        const barPx = barWidthPx(u.bbox.width / ratio, xs.length);
        const totals = showsTotalLabel(barPx);
        const baseline = u.valToPos(0, 'y', true);

        const ctx = u.ctx;
        ctx.save();
        ctx.textAlign = 'center';

        for (let index = 0; index < xs.length; index += 1) {
          const at = xs[index];
          if (at === null || at === undefined) continue;
          ctx.globalAlpha =
            hover.index === null || hover.index === index ? 1 : DIM_ALPHA;
          const centre = u.valToPos(at, 'x', true);
          const total = queries[index];
          const dropped = blocked[index];

          if (totals && total !== null && total !== undefined) {
            const top = u.valToPos(total, 'y', true);
            const y = top - TOTAL_LABEL_GAP_PX * ratio;
            // Dropped rather than shrunk here too: a label with no room above
            // its bar would otherwise be drawn over the axis.
            if (y - TOTAL_LABEL_SIZE_PX * ratio >= u.bbox.top) {
              ctx.font = `${String(TOTAL_LABEL_SIZE_PX * ratio)}px ${theme.mono}`;
              ctx.fillStyle = theme.barLabel;
              ctx.textBaseline = 'bottom';
              ctx.fillText(compactCount(total), centre, y);
            }
          }

          if (dropped === null || dropped === undefined || dropped <= 0) continue;
          const segmentTop = u.valToPos(dropped, 'y', true);
          if (!showsSegmentLabel((baseline - segmentTop) / ratio, barPx)) {
            continue;
          }
          ctx.font = `500 ${String(SEGMENT_LABEL_SIZE_PX * ratio)}px ${theme.mono}`;
          ctx.fillStyle = theme.segmentLabel;
          ctx.textBaseline = 'middle';
          ctx.fillText(
            compactCount(dropped),
            centre,
            (segmentTop + baseline) / 2,
          );
        }

        ctx.restore();
      },
    },
  };
}

/* -------------------------------------------------------------- hover plugin */

const TOOLTIP_OFFSET_PX = 14;

export interface HoverPluginInput {
  resolution: HistoryResolution;
  hover: HoverState;
  /**
   * The response item behind a bar. Read through a callback so the options
   * object survives a new range's data without being rebuilt — the plot is
   * keyed on that object's identity.
   */
  item: (index: number) => HoverItem | null;
}

/**
 * The only hover state in the system: a dark tooltip carrying the bucket
 * window, `queries`, `blocked` and the **served** `blocked_percent`, with the
 * hovered bar at full opacity and the rest dimmed.
 *
 * The tooltip is a DOM node rather than canvas painting — it is text a reader
 * may want to select, it inherits the theme tokens, and it can overflow the
 * plot without being clipped.
 */
export function hoverPlugin(input: HoverPluginInput): uPlot.Plugin {
  const { resolution, hover, item } = input;
  let node: HTMLDivElement | null = null;

  const hide = () => {
    if (node !== null) node.style.display = 'none';
  };

  return {
    hooks: {
      init: (u: uPlot) => {
        node = document.createElement('div');
        node.className = 'chart-tip';
        node.style.display = 'none';
        u.over.appendChild(node);
      },
      setCursor: (u: uPlot) => {
        const index = u.cursor.idx ?? null;
        const changed = index !== hover.index;
        hover.index = index;

        if (index === null || node === null) {
          hide();
          // Paths carry the per-bar fills, so the un-dim needs them rebuilt.
          if (changed) u.redraw(true, false);
          return;
        }

        const entry = item(index);
        if (entry === null) {
          hide();
          if (changed) u.redraw(true, false);
          return;
        }

        node.innerHTML = '';
        node.appendChild(row(bucketWindowLabel(entry.ts, resolution), null));
        node.appendChild(row('queries', entry.queries.toLocaleString()));
        node.appendChild(
          row('blocked', entry.blocked.toLocaleString(), 'blocked'),
        );
        node.appendChild(row('blocked %', percent1(entry.blockedPercent)));

        node.style.display = 'block';
        const ratio = pixelRatio();
        const centre = u.valToPos(u.data[0][index] ?? 0, 'x', true) / ratio;
        const plotWidth = u.bbox.width / ratio;
        const left = u.bbox.left / ratio;
        const flip = centre + TOOLTIP_OFFSET_PX + node.offsetWidth > left + plotWidth;
        node.style.left = `${String(
          flip
            ? centre - left - TOOLTIP_OFFSET_PX - node.offsetWidth
            : centre - left + TOOLTIP_OFFSET_PX,
        )}px`;
        node.style.top = '8px';

        if (changed) u.redraw(true, false);
      },
      destroy: () => {
        node?.remove();
        node = null;
      },
    },
  };
}

function row(label: string, value: string | null, tone?: string): HTMLElement {
  const line = document.createElement('div');
  line.className = value === null ? 'chart-tip-head' : 'chart-tip-row';
  const name = document.createElement('span');
  name.textContent = label;
  line.appendChild(name);
  if (value !== null) {
    const figure = document.createElement('span');
    figure.textContent = value;
    if (tone !== undefined) figure.className = tone;
    line.appendChild(figure);
  }
  return line;
}
