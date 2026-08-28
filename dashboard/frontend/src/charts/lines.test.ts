import type uPlot from 'uplot';
import { describe, expect, it } from 'vitest';
import { lineChartOptions } from './lines';
import { withAlpha } from './scale';
import type { ChartTheme } from './theme';

/**
 * jsdom has no canvas 2D context, so nothing here draws — these assert the
 * options object the wrapper hands to uPlot, which is where every property the
 * task depends on actually lives.
 */

const THEME: ChartTheme = {
  permitted: '#1f9dbb',
  blocked: '#d1504b',
  series1: '#1f9dbb',
  series2: '#3d9a63',
  series4: '#8a95a3',
  series5: '#6d5fa6',
  budget: '#dd9a2f',
  memoryRuleset: '#2a78d6',
  memoryCache: '#0b6647',
  memoryStats: '#4fc99a',
  memoryPeak: '#eda100',
  memoryWatch: '#a67c00',
  memoryOver: '#d03b3b',
  memoryResidualFill: '#e3e8ee',
  memoryResidualLine: '#a8b3c0',
  ink: '#1f2733',
  grid: '#eef2f6',
  axis: '#cfd8e3',
  tick: '#8a95a3',
  barLabel: '#47535f',
  segmentLabel: '#fff',
  surface: '#ffffff',
  font: 'system-ui',
  mono: 'monospace',
};

function options(
  series: Parameters<typeof lineChartOptions>[0]['series'],
  budget?: { value: number; label: string },
) {
  return lineChartOptions({
    theme: THEME,
    series,
    format: (value) => (value === 0 ? null : String(value)),
    ...(budget === undefined ? {} : { budget }),
  });
}

/** uPlot's `series[0]` is the x series and carries no spec. */
function drawn(config: ReturnType<typeof options>): uPlot.Series[] {
  return (config.series ?? []).slice(1) as uPlot.Series[];
}

describe('a series spec', () => {
  it('draws p99 solid and p50 dashed and lighter, so a pair reads as one stage', () => {
    const config = options([
      { label: 'block p99', colour: THEME.series2 },
      { label: 'block p50', colour: THEME.series2, dash: true },
    ]);
    const [solid, dashed] = drawn(config);
    expect(solid?.dash).toBeUndefined();
    expect(solid?.stroke).toBe(THEME.series2);
    expect(dashed?.dash).toEqual([4, 3]);
    expect(dashed?.stroke).toBe(withAlpha(THEME.series2, 0.55));
  });

  it('fills to the baseline only when the spec asks for an area', () => {
    const config = options([
      { label: 'qps', colour: THEME.series1, area: true },
      { label: 'p99', colour: THEME.series1 },
    ]);
    const [area, line] = drawn(config);
    expect(area?.fill).toBe(withAlpha(THEME.series1, 0.22));
    expect(line?.fill).toBeUndefined();
  });

  it('never bridges a gap on any series', () => {
    // An exact `0.0` latency sample means no traffic in that stage in that
    // interval. Bridging the `null` would draw a dip that never happened.
    const config = options([
      { label: 'a', colour: THEME.series1 },
      { label: 'b', colour: THEME.series2, dash: true },
      { label: 'c', colour: THEME.series5, area: true },
    ]);
    for (const series of drawn(config)) {
      expect(series.spanGaps).toBe(false);
    }
  });
});

describe('the budget marker', () => {
  it('is one draw hook and no data series', () => {
    const config = options([{ label: 'p99', colour: THEME.series2 }], {
      value: 1,
      label: '1.0 ms — budget',
    });
    expect(config.plugins).toHaveLength(1);
    expect(config.plugins?.[0]?.hooks.draw).toBeTypeOf('function');
    // A budget series would join the legend and read as enforced. There is one
    // x series and one data series, and that is all.
    expect(config.series).toHaveLength(2);
  });

  it('is absent entirely when the chart has no budget', () => {
    const config = options([{ label: 'qps', colour: THEME.series1 }]);
    expect(config.plugins).toBeUndefined();
  });

  it('keeps the marker inside the y range even when every reading is far under', () => {
    const config = options([{ label: 'p99', colour: THEME.series2 }], {
      value: 1,
      label: '1.0 ms — budget',
    });
    const range = config.scales?.['y']?.range as (
      u: unknown,
      min: number,
      max: number,
    ) => [number, number];
    expect(range(null, 0, 0.04)).toEqual([0, 1]);
  });

  it('still scales to the data when a reading is over the budget', () => {
    const config = options([{ label: 'p99', colour: THEME.series2 }], {
      value: 1,
      label: '1.0 ms — budget',
    });
    const range = config.scales?.['y']?.range as (
      u: unknown,
      min: number,
      max: number,
    ) => [number, number];
    expect(range(null, 0, 3.4)).toEqual([0, 4]);
  });
});

describe('the axes', () => {
  it('prints no x ticks — the span is named in words beneath the plot', () => {
    const config = options([{ label: 'qps', colour: THEME.series1 }]);
    expect(config.axes?.[0]?.show).toBe(false);
  });

  it('formats the y ticks through the caller’s own unit', () => {
    const ms = lineChartOptions({
      theme: THEME,
      series: [{ label: 'p99', colour: THEME.series2 }],
      format: (value) => (value === 0 ? null : `${value.toFixed(2)}`),
      budget: { value: 1, label: '1.0 ms — budget' },
    });
    const values = ms.axes?.[1]?.values as (
      u: unknown,
      splits: number[],
    ) => (string | null)[];
    expect(values(null, [0, 0.25, 0.5, 0.75, 1])).toEqual([
      null,
      '0.25',
      '0.50',
      '0.75',
      '1.00',
    ]);
  });
});
