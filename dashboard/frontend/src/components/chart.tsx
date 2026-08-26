import { useEffect, useRef, useState } from 'preact/hooks';
import type uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';

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
 * The only module that imports uPlot, and it imports it dynamically — one
 * static import anywhere collapses the split silently and puts the chart
 * library on the login path.
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

    void import('uplot')
      .then(({ default: UPlot }) => {
        if (!live || host.current === null) return;
        const width = node.clientWidth || 600;
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
