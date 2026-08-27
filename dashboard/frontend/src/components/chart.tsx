import { useEffect, useRef, useState } from 'preact/hooks';
import type uPlot from 'uplot';
import { loadUPlot } from '../charts/runtime';
// uPlot's vendor stylesheet is deliberately **not** imported: `cssCodeSplit` is
// `false`, so it would join the one stylesheet the login path fetches. The
// rules this chart actually reaches are written under `.chart` in
// `styles/components.css` — see the comment there before enabling any uPlot
// feature this task turned off.

export interface ChartProps {
  /** uPlot's aligned-data shape: `[xs, ...series]`. New data is pushed into the
   *  existing plot with `setData`; it never rebuilds one. */
  data: uPlot.AlignedData;
  /**
   * **Must be referentially stable** — hoist it to a module constant or wrap it
   * in `useMemo`. It keys the instance, so a fresh object literal every render
   * would destroy and rebuild the plot on every render, losing whatever state
   * uPlot holds.
   */
  options: Omit<uPlot.Options, 'width' | 'height'>;
  height?: number;
  /**
   * Set when the API returned `stride > 1`. Decimation is visible: every bar is
   * a real reading, never an average.
   */
  decimatedBy?: number;
}

/**
 * How many uPlot instances this session has constructed. p5-05's finding m4 —
 * that a caller passing an inline `options` object rebuilds the plot on every
 * render — has no jsdom proof, because uPlot needs a canvas 2D context. This
 * counter is that proof's instrument: a `stats` push re-renders the Dashboard
 * every ~2 s and must move it by zero, while a range change must move it by
 * exactly one.
 */
let constructions = 0;

export function chartConstructions(): number {
  return constructions;
}

/**
 * The wrapper every chart in the phase goes through. It reaches uPlot only
 * through `charts/runtime.ts`, which owns the single dynamic import and is what
 * keeps the library off the login path.
 */
export function Chart({ data, options, height = 220, decimatedBy }: ChartProps) {
  const host = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const [failed, setFailed] = useState(false);
  // Read at creation time, which is one dynamic import later than this render.
  const latestData = useRef(data);
  latestData.current = data;

  useEffect(() => {
    let live = true;
    const node = host.current;
    if (node === null) return;

    let observer: ResizeObserver | null = null;

    void loadUPlot()
      .then((UPlot) => {
        if (!live || host.current === null) return;
        const width = node.clientWidth || 600;
        constructions += 1;
        plot.current = new UPlot(
          { ...options, width, height } as uPlot.Options,
          latestData.current,
          node,
        );
        observer = new ResizeObserver(() => {
          plot.current?.setSize({ width: node.clientWidth || width, height });
        });
        observer.observe(node);
      })
      .catch(() => {
        if (live) setFailed(true);
      });

    return () => {
      live = false;
      observer?.disconnect();
      plot.current?.destroy();
      plot.current = null;
    };
  }, [options, height]);

  // New readings go into the existing plot. Keying the instance on `data` would
  // rebuild the whole chart every time a refresh lands.
  useEffect(() => {
    plot.current?.setData(data);
  }, [data]);

  // Dev-only, and the dead branch takes the assignment with it in a production
  // build: the construction count has to be readable from a browser console or
  // a driver, which a module-local export is not.
  if (import.meta.env.DEV) {
    (window as unknown as Record<string, unknown>)['fahChartBuilds'] =
      chartConstructions;
  }

  return (
    <div>
      <div class="chart" ref={host} />
      {failed && <p class="chart-footnote">The chart could not be drawn.</p>}
      {decimatedBy !== undefined && decimatedBy > 1 && (
        <p class="chart-footnote">
          Decimated: one point every {decimatedBy} buckets. Every point is a real
          reading, never an average.
        </p>
      )}
    </div>
  );
}
