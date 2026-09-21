import type uPlot from 'uplot';
import type { ChartTheme } from '../../charts/theme';
import { loadedUPlot } from '../../charts/runtime';
import { formatMiB } from '../../charts/format';
import { restartIndices } from '../../derive';
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
 * The MiB scale, drawn on `side` — 3 for the left edge, 1 for the right.
 *
 * One definition for both edges. The two axes have to agree on their splits and
 * their formatting or the plot reads differently depending on which edge you
 * look at, and two copies of the same object is exactly how they would stop
 * agreeing. `scale` is stated rather than inferred because uPlot only infers it
 * for the first two axes, and only the left one draws the grid: a second set of
 * lines over identical splits doubles every rule.
 */
function mibAxis(theme: ChartTheme, side: 1 | 3): uPlot.Axis {
  return {
    side,
    scale: 'y',
    stroke: theme.tick,
    grid: side === 3 ? { stroke: theme.grid, width: 1 } : { show: false },
    ticks: { show: false },
    font: `12px ${theme.mono}`,
    size: 62,
    splits: (u) => ySplits(u.scales['y']?.max ?? 0),
    values: (_u, splits) =>
      splits.map((value) => (value === 0 ? '' : `${(value / MIB).toFixed(0)} MiB`)),
  };
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
 *
 * `faults` is never drawn. It rides along on its own scale so `restartsOf` can
 * read the fault counter beside the peak, off the same data the line is drawn
 * from.
 */
const SERIES = {
  x: 0,
  ruleset: 1,
  cache: 2,
  stats: 3,
  rss: 4,
  peak: 5,
  faults: 6,
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
 * The rule itself lives in `restartIndices` — a fall in the high-water mark or
 * in the fault counter — and is shared with the residual verdict so the marker
 * and the verdict cannot disagree about where a process ended.
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
  const faults = u.data[SERIES.faults];
  if (peaks === undefined || faults === undefined) return [];
  return restartIndices(
    peaks as ArrayLike<number | null | undefined>,
    faults as ArrayLike<number | null | undefined>,
  );
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

/**
 * The last sampled RSS and peak, printed against the right scale in the colour
 * of the line each belongs to — peak in its own amber, and RSS in whichever of
 * the three state colours its line is wearing at that sample: ink under the
 * watch line, amber above it, red above the steady-state budget.
 *
 * The state comes from `rssStates`, the page's one state decision, so the
 * reading carries the same 3 MiB hysteresis the line does and the two can never
 * disagree about a sample. Re-deriving it from the value alone would strobe on
 * the boundary the line is steady on, which is the bug the walk exists to stop.
 *
 * The last point can be null on either series: a sample that recorded one and
 * not the other is a gap in that line, and a reading printed for a line that is
 * not there would be the older value wearing the current one's place.
 *
 * When the two are within a line of each other the peak is nudged up, so the
 * pair never overprints — which is exactly the case that matters, a process
 * sitting at its own high-water mark.
 */
function currentReadings(u: uPlot, theme: ChartTheme, ratio: number): void {
  const ctx = u.ctx;
  const right = u.bbox.left + u.bbox.width;
  const last = (u.data[SERIES.x]?.length ?? 0) - 1;
  if (last < 0) return;

  const rss = u.data[SERIES.rss];
  const state = rss === undefined ? null : (rssStates(rss)[last] ?? null);
  const statePaint: Record<RssState, string> = {
    normal: theme.ink,
    watch: theme.memoryWatch,
    over: theme.memoryOver,
  };

  const readings = [
    {
      value: rss?.[last],
      colour: state === null ? theme.ink : statePaint[state],
    },
    { value: u.data[SERIES.peak]?.[last], colour: theme.memoryPeak },
  ]
    .filter(
      (entry): entry is { value: number; colour: string } =>
        typeof entry.value === 'number',
    )
    .map((entry) => ({
      ...entry,
      // The figure alone. The axis beside it is already labelled in MiB, and
      // repeating the unit on every reading says nothing the scale has not.
      text: (entry.value / MIB).toFixed(1),
      y: Math.round(u.valToPos(entry.value, 'y', true)),
    }))
    .sort((a, b) => a.y - b.y);

  const minGap = (LABEL_PX + 3) * ratio;
  if (readings.length === 2 && readings[1]!.y - readings[0]!.y < minGap) {
    readings[0]!.y = readings[1]!.y - minGap;
  }

  ctx.font = `600 ${String(LABEL_PX * ratio)}px ${theme.mono}`;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'middle';
  for (const reading of readings) {
    const x = right + 6 * ratio;
    plate(ctx, theme, ratio, {
      text: reading.text,
      x,
      y: reading.y + LABEL_PX * ratio * 0.5,
      align: 'left',
    });
    ctx.fillStyle = reading.colour;
    ctx.fillText(reading.text, x, reading.y);
  }
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

        // The two current readings, against the right scale and each in the
        // colour of the line it belongs to. They sit in the gutter the right
        // axis draws, over its tick labels — the plate clears one, which is the
        // trade: a tick is a round number the reader can infer from its
        // neighbours, and these two are the figures the chart exists to report.
        //
        // Drawn from the last sample rather than from the readout above, so the
        // number and the line it names cannot disagree.
        currentReadings(u, theme, ratio);

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

            // The marker is the dashed rule and this dot, and no more. Three
            // sentences used to be drawn over the plot at every restart — what
            // the drop means, that it is never a reclaim, and the new peak's
            // figure — and a window with several restarts in it was more
            // caption than series. The footnote under the chart carries the
            // same facts once, where they do not sit on the data.
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
        // Every tick, including the last. The final label used to be given up
        // to a `now` drawn hard against the right edge, which named the end of
        // the window — but the window ends at the last *sample*, up to a
        // sampling interval before now, so `now` was a claim the series could
        // not support and the tick it displaced was a time that was sampled.
        values: (_u, splits) =>
          splits.map((value) => format.format(new Date(value * 1000))),
      },
      // The MiB scale on both edges. The plot is wide enough that a value near
      // the right edge is a long way from the axis that reads it, and the eye
      // has to track back across the whole series to place it. One definition,
      // called twice: the two have to agree on splits and formatting, and two
      // copies is how they would stop agreeing.
      mibAxis(theme, 3),
      mibAxis(theme, 1),
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
      [SERIES.faults]: {
        label: 'faults',
        scale: 'faults',
        show: false,
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
