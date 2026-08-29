import { describe, expect, it } from 'vitest';
import { niceMax, ySplits } from './scale';
import {
  HOUR_AXIS_LABELS,
  SEGMENT_LABEL_MIN_PX,
  TOTAL_LABEL_MIN_BAR_PX,
  barWidthPx,
  hourSplits,
  showsSegmentLabel,
  showsTotalLabel,
} from './stacked-bars';

describe('the y scale', () => {
  it('rounds up to the bound the artboard draws', () => {
    // 13,169 is `Main.dc.html`'s tallest bar and its axis tops out at 15k.
    expect(niceMax(13169)).toBe(15000);
  });

  it('never rounds down below the data', () => {
    for (const max of [1, 9, 10, 99, 101, 4021, 512883, 18639283]) {
      expect(niceMax(max)).toBeGreaterThanOrEqual(max);
    }
  });

  it('gives an all-zero range a readable scale rather than a zero one', () => {
    expect(niceMax(0)).toBe(4);
    expect(ySplits(0)).toEqual([0, 1, 2, 3, 4]);
  });

  it('draws five gridlines, evenly spaced, starting at zero', () => {
    const splits = ySplits(13169);
    expect(splits).toHaveLength(5);
    expect(splits[0]).toBe(0);
    expect(splits[4]).toBe(15000);
    expect(splits[1]).toBe(3750);
  });
});

/**
 * The plot area at each breakpoint, not the viewport: chrome comes off first.
 *
 *   1400 → 230 sidebar + 40 wrap + 28 card + 46 y-axis = 344
 *    900 →  64 sidebar (icons below 1200) + 40 + 28 + 46 = 178
 *    390 →   0 sidebar (drawer) + 24 wrap + 24 card + 46 = 94
 *
 * These are the widths the floors are actually asked about, and the 390 px row
 * is what `MobileDashboard.dc.html` states about itself: ~11 px per bar.
 */
const PLOT_PX = { desktop: 1056, tablet: 722, phone: 296 };

describe('the printed-figure floors', () => {
  it('reproduces the phone artboard’s own measurement', () => {
    expect(barWidthPx(PLOT_PX.phone, 24)).toBeCloseTo(11.1, 1);
  });

  it('prints no bar figure at any bucket count on a phone', () => {
    for (const buckets of [24, 7, 30]) {
      expect(showsTotalLabel(barWidthPx(PLOT_PX.phone, buckets))).toBe(false);
    }
  });

  it('prints figures only where a bar is actually wide enough', () => {
    const table: Array<[number, number, boolean]> = [
      [PLOT_PX.desktop, 24, false],
      [PLOT_PX.desktop, 7, true],
      [PLOT_PX.desktop, 30, false],
      [PLOT_PX.tablet, 24, false],
      [PLOT_PX.tablet, 7, true],
      [PLOT_PX.tablet, 30, false],
    ];
    for (const [width, buckets, expected] of table) {
      expect(showsTotalLabel(barWidthPx(width, buckets))).toBe(expected);
    }
  });

  it('prints figures on 24 bars once the plot is wide enough for them', () => {
    // A 1920 px window: 1920 − 344 of chrome.
    expect(showsTotalLabel(barWidthPx(1576, 24))).toBe(true);
  });

  it('caps a bar so a two-bucket range does not draw two slabs', () => {
    expect(barWidthPx(1056, 2)).toBe(60);
  });

  it('is empty rather than infinite with no buckets', () => {
    expect(barWidthPx(1056, 0)).toBe(0);
    expect(showsTotalLabel(0)).toBe(false);
  });

  it('drops a segment figure below the floor rather than shrinking it', () => {
    const wide = TOTAL_LABEL_MIN_BAR_PX;
    expect(showsSegmentLabel(SEGMENT_LABEL_MIN_PX, wide)).toBe(true);
    expect(showsSegmentLabel(SEGMENT_LABEL_MIN_PX - 0.1, wide)).toBe(false);
    expect(showsSegmentLabel(0, wide)).toBe(false);
  });

  it('never prints a segment figure wider than the bar it is inside', () => {
    // A tall segment on a 20 px bar: the figure would run into its neighbours.
    expect(showsSegmentLabel(40, 20)).toBe(false);
    expect(showsSegmentLabel(40, barWidthPx(PLOT_PX.phone, 24))).toBe(false);
  });

  it('treats each floor as inclusive at exactly its value', () => {
    expect(showsTotalLabel(TOTAL_LABEL_MIN_BAR_PX)).toBe(true);
    expect(showsTotalLabel(TOTAL_LABEL_MIN_BAR_PX - 0.1)).toBe(false);
  });
});

describe('the hourly x axis', () => {
  const HOUR = 3600;
  /** A 24 h window opening at 10:00 UTC — `Main.dc.html`'s own. */
  const TEN = Date.UTC(2026, 7, 26, 10) / 1000;

  const clock = (at: number) =>
    new Date(at * 1000).toISOString().slice(11, 16);

  it('prints the plan’s four, whatever the phase of the window', () => {
    // The defect this replaces: uPlot chose the increment from its own table
    // and emitted however many landed inside the window, so the same range at
    // the same width gave 3, 4 or 5 depending on where midnight fell.
    for (let openingHour = 0; openingHour < 24; openingHour += 1) {
      const min = Date.UTC(2026, 7, 26, openingHour) / 1000;
      expect(hourSplits(min, min + 23 * HOUR)).toHaveLength(HOUR_AXIS_LABELS);
    }
  });

  it('is the artboard’s cadence on the artboard’s window', () => {
    expect(hourSplits(TEN, TEN + 23 * HOUR).map(clock)).toEqual([
      '10:00',
      '16:00',
      '22:00',
      '04:00',
    ]);
  });

  it('survives the padding uPlot puts either side of a bar series', () => {
    // The scale is asked, not the data: for bars uPlot widens the range by
    // half a slot, so `min` is not a whole hour.
    expect(hourSplits(TEN - 1800, TEN + 23 * HOUR + 1800).map(clock)).toEqual([
      '10:00',
      '16:00',
      '22:00',
      '04:00',
    ]);
  });

  it('lands every split on a whole hour', () => {
    for (const at of hourSplits(TEN + 137, TEN + 23 * HOUR)) {
      expect(at % HOUR).toBe(0);
    }
  });

  it('never steps below an hour on a short window', () => {
    // Four buckets is not a range the chips offer; it is what a decimated or
    // truncated response could be, and a 15-minute label is not a clock the
    // rest of the page uses.
    const splits = hourSplits(TEN, TEN + 3 * HOUR);
    expect(splits).toHaveLength(HOUR_AXIS_LABELS);
    expect(splits.map(clock)).toEqual(['10:00', '11:00', '12:00', '13:00']);
  });

  it('emits one split rather than dividing an empty window', () => {
    expect(hourSplits(TEN, TEN)).toEqual([TEN]);
  });
});
