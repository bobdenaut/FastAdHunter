import type uPlot from 'uplot';
import type { ChartTheme } from '../../charts/theme';
import { loadedUPlot } from '../../charts/runtime';
import { formatMiB } from '../../charts/format';
import {
  STEADY_STATE_BUDGET,
  WATCH_THRESHOLD,
  rssStates,
  type RssState,
} from './budgets';
import type { RangeKey } from '../dashboard/ranges';

/**
 * The Memory trend's own options, deliberately not `charts/lines.ts`.
 *
 * That builder serves Performance, and three of its decisions are ones this
 * chart has to reverse: it draws no x ticks, it carries no tooltip, and it
 * knows one budget marker. Widening it to cover both would put four flags on a
 * shared module and give Performance a hover layer it decided against. The
 * duplication here is the y-scale and the axis colours; the reasoning is not
 * shared, so the module is not either.
 */

const MIB = 1024 * 1024;

/**
 * The ladder the y ceiling steps through, in MiB.
 *
 * Stepped rather than fitted, for two reasons. A fitted axis relabels itself on
 * every refresh, so two readings minutes apart cannot be compared and 24 h
 * cannot be compared against 30 d. And a fitted axis makes a flat line look
 * violent: RSS wobbling half a MiB would fill the card, which is the worst
 * possible failure on the one page whose question is "is residual trending up".
 */
const CEILINGS = [64, 96, 128, 160, 192, 256, 320, 512, 768, 1024];

function niceCeiling(bytes: number): number {
  const mib = (bytes * 1.05) / MIB;
  const step = CEILINGS.find((candidate) => candidate >= mib);
  return (step ?? Math.ceil(mib / 256) * 256) * MIB;
}

/** Quarters of the ceiling, so the four gridlines are always round. */
function ySplits(max: number): number[] {
  return [0, 0.25, 0.5, 0.75, 1].map((fraction) => max * fraction);
}

/**
 * The plot's column layout, named once.
 *
 * `u.data` is positional and every reader of it was writing its own index —
 * `u.data[5]` for peak, `u.data[4]` for RSS. Adding a series silently re-pointed
 * restart detection and the state stroke at the wrong line, and nothing in the
 * suite asserts the order. The series array below is built from these names, so
 * the constant and the layout cannot drift: change one and the other moves with
 * it.
 */
const SERIES = {
  x: 0,
  ruleset: 1,
  cache: 2,
  stats: 3,
  rss: 4,
  peak: 5,
} as const;

/** The slots above, back in index order, with every one of them filled. */
function seriesByName(
  slots: Record<(typeof SERIES)[keyof typeof SERIES], uPlot.Series>,
): uPlot.Series[] {
  return Object.values(SERIES)
    .slice()
    .sort((left, right) => left - right)
    .map((index) => slots[index]);
}

/**
 * The two paints the RSS series asks for on **every** draw.
 *
 * uPlot calls `stroke` and `fill` per draw, and per cursor move — so a hover
 * sweep was building a `<canvas>`, a `CanvasPattern` and a `CanvasGradient`
 * each time, for paints that change only when the theme, the device ratio, the
 * plot box or the data does. They are cached against exactly those inputs and
 * rebuilt when one moves. Held per options object, so it dies with the plot it
 * belongs to and no two instances share a canvas.
 */
interface PaintCache {
  hatch: { ratio: number; paint: CanvasPattern | string } | null;
  stroke: {
    values: unknown;
    left: number;
    width: number;
    paint: CanvasGradient | string;
  } | null;
}

const HATCH_PX = 7;
const RULE_DASH = [1, 4];
/** Annotation and threshold-caption size. 9 px was unreadable on the chart at
 *  a normal viewing distance; the axis runs 11 px and these read alongside it. */
const LABEL_PX = 13;
/** How much room `now` needs at the right edge, in CSS pixels. */
const NOW_GUTTER = 34;

function pixelRatio(): number {
  return loadedUPlot()?.pxRatio ?? 1;
}

/**
 * The residual band's texture, built once per theme.
 *
 * Residual is a remainder rather than a structure, so it is the one band that
 * never takes a hue — and a texture is also the only encoding left once the
 * three categorical hues, the amber and the red are spoken for.
 */
function hatchPattern(
  ctx: CanvasRenderingContext2D,
  theme: ChartTheme,
  cache: PaintCache,
): CanvasPattern | string {
  const ratio = pixelRatio();
  // The theme is fixed for the life of this options object — `useChartTheme`
  // rebuilds it on a theme change — so the ratio is the only input left.
  const held = cache.hatch;
  if (held !== null && held.ratio === ratio) return held.paint;

  const tile = document.createElement('canvas');
  const size = HATCH_PX * pixelRatio();
  tile.width = size;
  tile.height = size;
  const paint = tile.getContext('2d');
  if (paint === null) return theme.memoryResidualFill;
  paint.fillStyle = theme.memoryResidualFill;
  paint.fillRect(0, 0, size, size);
  paint.strokeStyle = theme.memoryResidualLine;
  paint.lineWidth = 2 * pixelRatio();
  paint.beginPath();
  paint.moveTo(0, size);
  paint.lineTo(size, 0);
  paint.moveTo(-size / 2, size / 2);
  paint.lineTo(size / 2, -size / 2);
  paint.moveTo(size / 2, size * 1.5);
  paint.lineTo(size * 1.5, size / 2);
  paint.stroke();
  const pattern = ctx.createPattern(tile, 'repeat') ?? theme.memoryResidualFill;
  cache.hatch = { ratio, paint: pattern };
  return pattern;
}

/**
 * RSS's stroke, one colour per state, with a hard edge at every sample the
 * state changes on.
 *
 * **The states come from `rssStates`, the page's one state decision, so the
 * line carries the 3 MiB hysteresis the sketch attributes to it.** The earlier
 * form keyed the gradient on the y scale, which cannot: a value on its own
 * does not say which side of a sticky threshold the series arrived from, so
 * that gradient re-decided the state from the raw thresholds and drew a line
 * that strobed on the boundary the card was steady on.
 *
 * Keying on x instead makes the colour a function of the sample, which is what
 * a per-segment stroke would be and uPlot has no other way to give. Paired
 * stops at one offset keep each change a hard edge — a blend would draw a
 * state that does not exist.
 */
function stateStroke(
  u: uPlot,
  ctx: CanvasRenderingContext2D,
  theme: ChartTheme,
  cache: PaintCache,
): CanvasGradient | string {
  const paint: Record<RssState, string> = {
    normal: theme.ink,
    watch: theme.memoryWatch,
    over: theme.memoryOver,
  };
  const values = u.data[SERIES.rss];
  const times = u.data[SERIES.x];
  if (values === undefined || times === undefined) return theme.ink;

  // The gradient is a function of the readings and the plot box, and of nothing
  // a cursor move changes — so a hover sweep reuses one rather than walking the
  // series per frame. `values` is compared by reference: `TrendCard` builds a
  // fresh `AlignedData` per data change and never mutates one in place.
  const held = cache.stroke;
  if (
    held !== null &&
    held.values === values &&
    held.left === u.bbox.left &&
    held.width === u.bbox.width
  ) {
    return held.paint;
  }
  const remember = (result: CanvasGradient | string) => {
    cache.stroke = {
      values,
      left: u.bbox.left,
      width: u.bbox.width,
      paint: result,
    };
    return result;
  };

  const states = rssStates(values);
  const first = states.find((state) => state !== null) ?? null;
  if (first === null) return remember(theme.ink);

  const left = u.bbox.left;
  const right = u.bbox.left + u.bbox.width;
  if (right <= left) return remember(paint[first]);

  const gradient = ctx.createLinearGradient(left, 0, right, 0);
  let current: RssState = first;
  gradient.addColorStop(0, paint[current]);
  for (let index = 0; index < states.length; index += 1) {
    const state = states[index];
    if (state === null || state === undefined || state === current) continue;
    const x = u.valToPos(times[index] ?? 0, 'x', true);
    const offset = Math.min(1, Math.max(0, (x - left) / (right - left)));
    gradient.addColorStop(offset, paint[current]);
    gradient.addColorStop(offset, paint[state]);
    current = state;
  }
  gradient.addColorStop(1, paint[current]);
  return remember(gradient);
}

/**
 * The watch zone and the two threshold rules, drawn under the data.
 *
 * **Both are dotted rules with captions, never walls**, and the red one is
 * never drawn without its caption: red is a status colour, and a status colour
 * that appears without the label naming its threshold is an alarm the reader
 * cannot check.
 */
/**
 * The restart rows, read off the plotted peak series (KTD7).
 *
 * `peak_rss` is `getrusage`'s high-water mark and is monotone within one
 * process lifetime, so a fall is a restart and never a reclaim. `null` rows are
 * skipped rather than read as a drop: they mean the row predates the field or
 * `getrusage` was unavailable, not that the peak was lower.
 *
 * **It reads `u.data`, not the page's `items`, on purpose.** The annotation has
 * to mark the line that is actually drawn. Deriving it from `items` and passing
 * the indices into the options would put data in an object the chart wrapper
 * keys its instance on, rebuilding the whole plot on every refresh (p5-05's
 * finding m4) — and would let the marker disagree with the line if the two ever
 * came from different renders.
 */
function restartsOf(u: uPlot): number[] {
  const peaks = u.data[SERIES.peak];
  if (peaks === undefined) return [];
  const out: number[] = [];
  let previous: number | null = null;
  for (let index = 0; index < peaks.length; index += 1) {
    const value = peaks[index] as number | null | undefined;
    if (value === null || value === undefined) continue;
    if (previous !== null && value < previous) out.push(index);
    previous = value;
  }
  return out;
}

/** Local midnight after `seconds`, in epoch seconds. */
function nextMidnight(seconds: number): number {
  const date = new Date(seconds * 1000);
  date.setHours(24, 0, 0, 0);
  return date.getTime() / 1000;
}

/**
 * The ground a canvas annotation clears for itself.
 *
 * Every label on this chart is drawn over five series and two rules, and the
 * peak line in particular sits wherever the data puts it — it struck the
 * threshold captions through whenever a restart happened to land near a
 * threshold. Canvas has no z-order for text, so the label paints the card
 * colour behind itself first. Sized from the measured text, so it is only ever
 * as wide as the words it is protecting.
 */
function plate(
  ctx: CanvasRenderingContext2D,
  theme: ChartTheme,
  ratio: number,
  at: { text: string; x: number; y: number; align: 'left' | 'right' },
): void {
  const width = ctx.measureText(at.text).width;
  const padX = 3 * ratio;
  const height = LABEL_PX * ratio + 2 * ratio;
  ctx.fillStyle = theme.surface;
  ctx.fillRect(
    (at.align === 'left' ? at.x : at.x - width) - padX,
    at.y - height + 2 * ratio,
    width + padX * 2,
    height,
  );
}

function thresholdPlugin(theme: ChartTheme, range: RangeKey): uPlot.Plugin {
  return {
    hooks: {
      drawClear: (u: uPlot) => {
        const ctx = u.ctx;
        const ratio = pixelRatio();
        const max = u.scales['y']?.max ?? 0;
        if (max < WATCH_THRESHOLD) return;

        const yOf = (value: number) => Math.round(u.valToPos(value, 'y', true));
        const left = u.bbox.left;
        const right = u.bbox.left + u.bbox.width;

        ctx.save();

        // The zone first, so both rules sit on top of their own shading.
        const zoneTop = yOf(Math.min(STEADY_STATE_BUDGET, max));
        const zoneBottom = yOf(WATCH_THRESHOLD);
        ctx.globalAlpha = 0.12;
        ctx.fillStyle = theme.memoryWatch;
        ctx.fillRect(left, zoneTop, right - left, zoneBottom - zoneTop);
        ctx.globalAlpha = 1;

        ctx.setLineDash(RULE_DASH.map((step) => step * ratio));
        ctx.font = `${String(LABEL_PX * ratio)}px ${theme.font}`;
        ctx.textAlign = 'left';
        ctx.textBaseline = 'bottom';

        // The rules only. Their captions are drawn in `draw`, over the series:
        // the peak line can sit at any height the data puts it, and where it
        // crossed a threshold it struck the caption through.
        const rule = (value: number, colour: string) => {
          if (value > max) return;
          const y = yOf(value);
          ctx.beginPath();
          ctx.strokeStyle = colour;
          ctx.lineWidth = 1.2 * ratio;
          ctx.moveTo(left, y);
          ctx.lineTo(right, y);
          ctx.stroke();
        };

        rule(WATCH_THRESHOLD, theme.memoryWatch);
        rule(STEADY_STATE_BUDGET, theme.memoryOver);

        ctx.setLineDash([]);

        // `now` at the right edge — an extra label beside the last tick, not a
        // tick relabelled. The window ends at the server's own now, which is
        // never a round clock time, so a tick there would be a time nobody
        // sampled at.
        ctx.font = `${String(LABEL_PX * ratio)}px ${theme.mono}`;
        ctx.fillStyle = theme.tick;
        ctx.textAlign = 'right';
        ctx.textBaseline = 'top';
        ctx.fillText('now', right, u.bbox.top + u.bbox.height + 8 * ratio);

        // Midnight, drawn stronger and named. On a 24 h window the reader is
        // looking at two calendar days and the axis alone does not say where
        // one ends — and on 7 d and 30 d every tick is a midnight, so the
        // emphasis would be meaningless and is not drawn.
        if (range === '24h') {
          const from = u.scales['x']?.min ?? 0;
          const to = u.scales['x']?.max ?? 0;
          const midnight = nextMidnight(from);
          if (midnight > from && midnight < to) {
            const x = Math.round(u.valToPos(midnight, 'x', true));
            ctx.beginPath();
            ctx.strokeStyle = theme.axis;
            ctx.lineWidth = 1 * ratio;
            ctx.moveTo(x, u.bbox.top);
            ctx.lineTo(x, u.bbox.top + u.bbox.height);
            ctx.stroke();
            ctx.font = `${String(LABEL_PX * ratio)}px ${theme.font}`;
            ctx.fillStyle = theme.tick;
            ctx.textAlign = 'center';
            ctx.textBaseline = 'top';
            // Below the tick labels, not level with them: `00:00` is drawn at
            // the axis and the caption belongs under it.
            ctx.fillText(
              'day boundary',
              x,
              u.bbox.top + u.bbox.height + 22 * ratio,
            );
          }
        }

        ctx.restore();
      },

      /**
       * The restart, named on the chart.
       *
       * **A fall in the peak line is the one event here a reader must not
       * mistake for a reclaim.** `peak_rss` is monotone within a process
       * lifetime, so it can only fall by the process being replaced — and the
       * new process's first peak is its startup compile, which lasts seconds
       * against a 60 s sample and therefore never appears in the area below.
       * Both facts are drawn, because a marker with no caption is a mystery and
       * a reader who guesses will guess "reclaim".
       *
       * Drawn in `draw` rather than `drawClear` so it sits over the bands.
       */
      draw: (u: uPlot) => {
        const ratio = pixelRatio();
        const ctx = u.ctx;
        const top = u.bbox.top;
        const bottom = u.bbox.top + u.bbox.height;
        const max = u.scales['y']?.max ?? 0;

        ctx.save();

        // The threshold captions, over the series and on their own ground.
        if (max >= WATCH_THRESHOLD) {
          ctx.font = `${String(LABEL_PX * ratio)}px ${theme.font}`;
          const caption = (value: number, colour: string, label: string) => {
            if (value > max) return;
            const y = Math.round(u.valToPos(value, 'y', true));
            plate(ctx, theme, ratio, {
              text: label,
              x: u.bbox.left + 8 * ratio,
              y: y - 5 * ratio,
              align: 'left',
            });
            ctx.fillStyle = colour;
            ctx.textAlign = 'left';
            ctx.textBaseline = 'bottom';
            ctx.fillText(label, u.bbox.left + 8 * ratio, y - 5 * ratio);
          };
          // Formatted from the constants the rules are drawn at, never written
          // out: a caption naming a threshold the line is not at is worse than
          // no caption, and red only ever appears beside the figure that
          // justifies it.
          caption(
            WATCH_THRESHOLD,
            theme.memoryWatch,
            `watch — ${formatMiB(WATCH_THRESHOLD)}`,
          );
          caption(
            STEADY_STATE_BUDGET,
            theme.memoryOver,
            `over budget — ${formatMiB(STEADY_STATE_BUDGET)}`,
          );
        }

        const restarts = restartsOf(u);
        if (restarts.length === 0) {
          ctx.restore();
          return;
        }
        for (const index of restarts) {
          const x = Math.round(u.valToPos(u.data[SERIES.x]?.[index] ?? 0, 'x', true));
          ctx.beginPath();
          ctx.strokeStyle = theme.tick;
          ctx.lineWidth = 1 * ratio;
          ctx.setLineDash([2 * ratio, 4 * ratio]);
          ctx.moveTo(x, top);
          ctx.lineTo(x, bottom);
          ctx.stroke();
          ctx.setLineDash([]);

          const peak = (u.data[SERIES.peak]?.[index] ?? null) as number | null;
          if (peak !== null) {
            const y = u.valToPos(peak, 'y', true);
            ctx.beginPath();
            ctx.fillStyle = theme.memoryPeak;
            ctx.arc(x, y, 3.5 * ratio, 0, Math.PI * 2);
            ctx.fill();

            ctx.font = `${String(LABEL_PX * ratio)}px ${theme.font}`;
            ctx.textAlign = 'right';
            ctx.textBaseline = 'alphabetic';
            ctx.fillStyle = theme.barLabel;
            // Each caption clears its own ground: the peak line runs level with
            // them on either side of the drop, and a dashed rule through the
            // words is the one thing that made this annotation unreadable.
            const say = (
              text: string,
              colour: string,
              at: { x: number; y: number; align: 'left' | 'right' },
            ) => {
              ctx.textAlign = at.align;
              plate(ctx, theme, ratio, { text, x: at.x, y: at.y, align: at.align });
              ctx.fillStyle = colour;
              ctx.fillText(text, at.x, at.y);
            };

            say('restart — the peak line drops here', theme.barLabel, {
              x: x - 8 * ratio,
              y: y - 22 * ratio,
              align: 'right',
            });
            say(
              'a fall in this line is always a restart, never a reclaim',
              theme.tick,
              { x: x - 8 * ratio, y: y - 8 * ratio, align: 'right' },
            );
            // Clamped to the plot's right edge. A restart near the end of the
            // window put this caption's tail outside the canvas, where it was
            // cut mid-word — the annotation that explains the drop is the last
            // thing that should be unreadable.
            const newPeak = `new peak ${(peak / MIB).toFixed(1)} — the startup compile, never sampled`;
            ctx.font = `${String(LABEL_PX * ratio)}px ${theme.font}`;
            const right = u.bbox.left + u.bbox.width;
            say(newPeak, theme.tick, {
              x: Math.min(
                x + 8 * ratio,
                right - ctx.measureText(newPeak).width - 4 * ratio,
              ),
              y: y + 22 * ratio,
              align: 'left',
            });
          }
        }
        ctx.restore();
      },
    },
  };
}

/**
 * How the x axis is labelled, per range.
 *
 * A 7 d window at `00:00` prints seven identical labels, so the format changes
 * with the span rather than only the tick spacing. Ticks land on round
 * boundaries rather than on `now`, so the labels are comparable between two
 * loads instead of shifting with the clock.
 */
const TICK_FORMAT: Record<RangeKey, Intl.DateTimeFormatOptions> = {
  '24h': { hour: '2-digit', minute: '2-digit', hour12: false },
  '7d': { weekday: 'short', day: 'numeric' },
  '30d': { month: 'short', day: 'numeric' },
};

export interface TrendOptionsInput {
  theme: ChartTheme;
  range: RangeKey;
  /** Rebuilds the tooltip's markup. Owned by the card, which knows the labels. */
  onCursor: (index: number | null) => void;
  /**
   * Positions the floating readout over the plot. `null` hides it.
   *
   * The box is a DOM node the card owns, not canvas: canvas text cannot be
   * selected, cannot be read by a screen reader, and would have to re-implement
   * font metrics to lay out two columns. Placement is the plugin's job because
   * only it knows the plot's box.
   *
   * **`x` is already in the positioned parent's coordinates**, left edge of the
   * box, clamped inside the card. An earlier version handed back a
   * plot-relative x and let the component choose `left:` or `right:` from a
   * `flip` flag — two coordinate systems and two anchors, which put the box a
   * y-axis width off and broke outright at the left edge. One number, one
   * anchor, computed where the geometry is known.
   */
  onPlace: (at: { x: number; y: number } | null) => void;
}

/** Kept in step with `.memory-tip` in `components.css`. */
const TIP_WIDTH = 196;
const TIP_GAP = 14;
/** Minimum breathing room between the box and the card edge. */
const TIP_EDGE = 6;

/**
 * **Must be memoised on `(range, theme)`.** The `Chart` wrapper keys the uPlot
 * instance on this object's identity, so a fresh literal per render would
 * destroy and rebuild the plot every render — p5-05's finding m4.
 */
export function memoryTrendOptions({
  theme,
  range,
  onCursor,
  onPlace,
}: TrendOptionsInput): Omit<uPlot.Options, 'width' | 'height'> {
  const format = new Intl.DateTimeFormat(undefined, TICK_FORMAT[range]);
  // One per options object, so it lives and dies with the plot the options key.
  const cache: PaintCache = { hatch: null, stroke: null };

  return {
    legend: { show: false },
    cursor: {
      x: true,
      y: false,
      drag: { x: false, y: false },
      points: { show: true, size: 7 },
    },
    scales: {
      x: { time: true },
      y: {
        // The scale always contains the steady-state budget: a process
        // comfortably inside it would otherwise scale to itself and put both
        // rules off the top, which is the one thing they exist to be seen
        // against. The ceiling then steps to a round number of MiB, so the
        // axis reads 40/80/120/160 rather than 24/48/72/95/119/143 — a scale
        // that lands on arbitrary values makes every reading an arithmetic
        // problem, and it moves on every refresh.
        range: (_u, _min, max) =>
          [0, niceCeiling(Math.max(max, STEADY_STATE_BUDGET))] as uPlot.Range.MinMax,
      },
    },
    axes: [
      {
        stroke: theme.tick,
        grid: { stroke: theme.grid, width: 1 },
        ticks: { stroke: theme.axis, width: 1, size: 5 },
        // Taller than the default so the `day boundary` caption has a line of
        // its own beneath the tick labels rather than sitting on them.
        size: 48,
        font: `12px ${theme.mono}`,
        // Room for eight-ish labels rather than twenty-four: a tick per hour is
        // a wall of digits nobody reads, and the artboard draws one every three.
        space: 130,
        // The last tick is dropped when it would sit under `now`, which the
        // threshold plugin draws hard against the right edge. Two labels in one
        // place read as one unreadable label, and `now` is the one that has to
        // survive: it names the end of the window, which no tick does.
        values: (u, splits) => {
          const right = u.bbox.left + u.bbox.width;
          return splits.map((value, index) => {
            const label = format.format(new Date(value * 1000));
            const collides =
              index === splits.length - 1 &&
              u.valToPos(value, 'x', true) > right - NOW_GUTTER * pixelRatio();
            return collides ? '' : label;
          });
        },
      },
      {
        side: 3,
        stroke: theme.tick,
        grid: { stroke: theme.grid, width: 1 },
        ticks: { show: false },
        font: `12px ${theme.mono}`,
        size: 62,
        splits: (u) => ySplits(u.scales['y']?.max ?? 0),
        values: (_u, splits) =>
          splits.map((value) =>
            value === 0 ? '' : `${(value / MIB).toFixed(0)} MiB`,
          ),
      },
    ],
    // Assigned by name rather than written in order: the slot each series
    // occupies is now stated where the series is defined, and it is the same
    // constant `restartsOf` and `stateStroke` read `u.data` with.
    series: seriesByName({
      [SERIES.x]: {},
      [SERIES.ruleset]: {
        label: 'ruleset',
        stroke: 'transparent',
        fill: theme.memoryRuleset,
        spanGaps: false,
        points: { show: false },
      },
      [SERIES.cache]: {
        label: 'cache',
        stroke: 'transparent',
        fill: theme.memoryCache,
        spanGaps: false,
        points: { show: false },
      },
      [SERIES.stats]: {
        label: 'stats',
        stroke: 'transparent',
        fill: theme.memoryStats,
        spanGaps: false,
        points: { show: false },
      },
      [SERIES.rss]: {
        label: 'RSS',
        // The top band is RSS, so its fill is the residual — the only slice
        // between `accounted` and the total — and its stroke is RSS itself.
        stroke: (u: uPlot) => stateStroke(u, u.ctx, theme, cache),
        fill: (u: uPlot) => hatchPattern(u.ctx, theme, cache),
        width: 2,
        spanGaps: false,
        points: { show: false },
      },
      [SERIES.peak]: {
        label: 'peak',
        stroke: theme.memoryPeak,
        width: 1.7,
        dash: [5, 3],
        spanGaps: false,
        points: { show: false },
      },
    }),
    bands: [
      { series: [SERIES.rss, SERIES.stats] },
      { series: [SERIES.stats, SERIES.cache] },
      { series: [SERIES.cache, SERIES.ruleset] },
    ],
    plugins: [
      thresholdPlugin(theme, range),
      {
        hooks: {
          setCursor: (u: uPlot) => {
            const index = u.cursor.idx ?? null;
            onCursor(index);
            if (index === null || u.cursor.left == null || u.cursor.left < 0) {
              onPlace(null);
              return;
            }
            // Snapped to the sample, not to the pointer: a box that tracks the
            // cursor freely names a timestamp no reading exists at.
            //
            // `valToPos` is relative to the plot area; the box is positioned
            // against the card, which also holds the y-axis gutter. `bbox` is
            // device pixels, so the gutter is converted back before it is added.
            const plotLeft = u.bbox.left / pixelRatio();
            const crosshair = plotLeft + u.valToPos(u.data[SERIES.x]?.[index] ?? 0, 'x');

            // **The side is chosen by the midpoint, not by whether the box
            // would fit.** A fit test flips at whatever x the box happens to
            // stop fitting at, which is a different x on every card width and
            // sits wherever the last label pushed it; the midpoint is the same
            // rule at every size, and the half the box moves into is by
            // construction the half with more room. It also cannot oscillate:
            // the box's own width never enters the decision, so it can never
            // flip into a position that makes it want to flip back.
            const preferred =
              crosshair > u.width / 2
                ? crosshair - TIP_GAP - TIP_WIDTH
                : crosshair + TIP_GAP;
            // The clamp is the safety net for a card too narrow for either
            // side to have room, not the placement rule.
            const x = Math.max(
              TIP_EDGE,
              Math.min(preferred, u.width - TIP_WIDTH - TIP_EDGE),
            );
            onPlace({ x, y: 12 });
          },
          // Leaving the plot has to clear it: uPlot fires no cursor event once
          // the pointer is gone, so without this the box stays behind.
          setSeries: (u: uPlot) => {
            if (u.cursor.idx == null) onPlace(null);
          },
        },
      },
    ],
  };
}
