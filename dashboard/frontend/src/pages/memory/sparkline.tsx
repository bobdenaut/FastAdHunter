import type { PerfItem } from '../../api/types';

/**
 * The plot box the KPI sparks share. `BASE` is below `TOP + HEIGHT` on purpose:
 * the area fill closes to the floor of the box, not to the lowest reading, so a
 * flat series still shows a band rather than a hairline.
 */
const WIDTH = 140;
const BASE = 38;
const TOP = 4;
const HEIGHT = 30;

interface Point {
  x: number;
  y: number;
}

/**
 * The window's readings as plot points, split into contiguous runs.
 *
 * `null` breaks a run rather than interpolating across it: a row without the
 * field is a gap in the record, and drawing through it would invent a
 * measurement. Both the line and the area are built from the same runs, so they
 * break in the same places.
 */
function runsOf(values: readonly (number | null)[]): Point[][] {
  const present = values.filter((value): value is number => value !== null);
  if (present.length < 2) return [];

  const min = Math.min(...present);
  const max = Math.max(...present);
  // A flat series would divide by zero; drawn mid-height, which is the truth.
  const span = max - min || 1;
  const step = values.length > 1 ? WIDTH / (values.length - 1) : WIDTH;

  const runs: Point[][] = [];
  let run: Point[] = [];
  values.forEach((value, index) => {
    if (value === null) {
      if (run.length > 0) runs.push(run);
      run = [];
      return;
    }
    run.push({
      x: index * step,
      y: TOP + HEIGHT - ((value - min) / span) * HEIGHT,
    });
  });
  if (run.length > 0) runs.push(run);
  return runs;
}

const point = (at: Point) => `${at.x.toFixed(1)},${at.y.toFixed(1)}`;

function linePath(runs: readonly Point[][]): string {
  return runs
    .map((run) => run.map((at, index) => `${index === 0 ? 'M' : 'L'}${point(at)}`).join(' '))
    .join(' ');
}

/** Each run closed down to the box floor, so a gap leaves a gap in the fill. */
function areaPath(runs: readonly Point[][]): string {
  return runs
    .filter((run) => run.length > 1)
    .map((run) => {
      const first = run[0] as Point;
      const last = run[run.length - 1] as Point;
      const along = run.map((at) => `L${point(at)}`).join(' ');
      return `M${first.x.toFixed(1)},${BASE.toFixed(1)} ${along} L${last.x.toFixed(1)},${BASE.toFixed(1)} Z`;
    })
    .join(' ');
}

function Frame({
  tone,
  children,
}: {
  tone: string;
  children: preact.ComponentChildren;
}) {
  return (
    <svg
      class={`kpi-spark kpi-spark-${tone}`}
      viewBox={`0 0 ${WIDTH} ${BASE}`}
      width={WIDTH}
      height={BASE}
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

/**
 * The trend beside a KPI figure — an inline SVG, not a uPlot instance.
 *
 * A plot per KPI would be four more canvases, four more `ResizeObserver`s and
 * four more constructions on every theme change, to draw twelve pixels of
 * shape that carries no axis, no hover and no readable value. The figure beside
 * it is the number; this is only the gesture, and the chart below is where the
 * trend is actually read.
 *
 * **The area under the line is a gradient to transparent**, as the artboard
 * draws it. It carries no reading of its own — the fill fades out before the
 * floor precisely so it is not read as a quantity — and it is what makes a
 * 30 px shape legible at a glance rather than a hairline. Its stops take the
 * line's own colour from CSS, so it follows the theme like everything else.
 *
 * The gradient id is derived from the tone, so two cards of the same tone would
 * share one definition rather than collide: same stops, same result.
 */
export function Sparkline({
  items,
  of,
  tone,
}: {
  items: readonly PerfItem[];
  of: (item: PerfItem) => number | null;
  tone: 'ink' | 'residual';
}) {
  const runs = runsOf(items.map(of));
  if (runs.length === 0) return null;

  const gradient = `kpi-spark-fill-${tone}`;
  return (
    <Frame tone={tone}>
      <defs>
        <linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1">
          <stop class="spark-stop-top" offset="0%" />
          <stop class="spark-stop-base" offset="100%" />
        </linearGradient>
      </defs>
      <path class="spark-area" d={areaPath(runs)} fill={`url(#${gradient})`} />
      <path class="spark-line" d={linePath(runs)} fill="none" stroke-width="1.6" />
    </Frame>
  );
}

/**
 * Peak RSS, which is a **step** rather than a curve.
 *
 * A high-water mark holds its value until something exceeds it and falls only
 * when the process restarts, so interpolating between two readings would draw a
 * slope the number never took. Drawn step-after — hold, then drop — and every
 * drop carries a dot, because a fall in this series is always a restart and
 * never a reclaim. That is the one thing a reader has to be able to see here.
 *
 * No area fill: the band under a high-water mark is not a quantity, and the
 * artboard draws this mark alone.
 */
export function PeakSpark({ items }: { items: readonly PerfItem[] }) {
  const runs = runsOf(
    items.map((item) =>
      item.peak_rss === undefined || item.peak_rss === 0 ? null : item.peak_rss,
    ),
  );
  if (runs.length === 0) return null;

  const segments: string[] = [];
  const drops: Point[] = [];
  for (const run of runs) {
    run.forEach((at, index) => {
      const previous = run[index - 1];
      if (previous === undefined) {
        segments.push(`M${point(at)}`);
        return;
      }
      // Hold the level to this sample's x, then move vertically to it.
      segments.push(`L${at.x.toFixed(1)},${previous.y.toFixed(1)}`);
      segments.push(`L${point(at)}`);
      // A larger y is a smaller reading — the counter went backwards, which
      // only a restart does.
      if (at.y > previous.y) drops.push(at);
    });
  }

  return (
    <Frame tone="peak">
      <path
        class="spark-line"
        d={segments.join(' ')}
        fill="none"
        stroke-width="1.6"
      />
      {drops.map((at) => (
        <circle
          class="spark-drop"
          key={`${at.x.toFixed(1)}-${at.y.toFixed(1)}`}
          cx={at.x.toFixed(1)}
          cy={at.y.toFixed(1)}
          r="2.6"
        />
      ))}
    </Frame>
  );
}
