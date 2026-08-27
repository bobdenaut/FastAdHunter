import { describe, expect, it } from 'vitest';
import {
  OTHER_LABEL,
  blockedPercent,
  queryTypeSlices,
  shareOfMax,
  sumOver,
  sumPerType,
  upstreamBar,
} from './derive';
import { compactCount, percent1 } from './charts/format';
import { formatUptime, lastRefreshLabel } from './time';

describe('compactCount', () => {
  it('prints the artboard’s figures exactly', () => {
    expect(compactCount(0)).toBe('0');
    expect(compactCount(412)).toBe('412');
    expect(compactCount(4021)).toBe('4k');
    expect(compactCount(1989)).toBe('2k');
    expect(compactCount(3200)).toBe('3.2k');
    expect(compactCount(11250)).toBe('11.3k');
    expect(compactCount(13169)).toBe('13.2k');
    expect(compactCount(15000)).toBe('15k');
  });

  it('carries on past a million rather than printing seven digits', () => {
    expect(compactCount(512883)).toBe('512.9k');
    expect(compactCount(18639283)).toBe('18.6M');
    expect(compactCount(2_400_000_000)).toBe('2.4G');
  });
});

describe('percent1', () => {
  it('keeps one decimal, trailing zero included', () => {
    expect(percent1(12.7)).toBe('12.7');
    expect(percent1(10)).toBe('10.0');
    expect(percent1(0)).toBe('0.0');
  });
});

describe('the range aggregate', () => {
  const items = [
    { queries: 100, blocked: 10 },
    { queries: 300, blocked: 20 },
  ];

  it('sums the same items that draw the bars', () => {
    expect(sumOver(items, (i) => i.queries)).toBe(400);
    expect(sumOver(items, (i) => i.blocked)).toBe(30);
  });

  it('is exact rather than an average of the per-bucket percentages', () => {
    expect(blockedPercent(400, 30)).toBeCloseTo(7.5);
  });

  it('is zero, not NaN, over an empty range', () => {
    expect(sumOver([], (i: { queries: number }) => i.queries)).toBe(0);
    expect(blockedPercent(0, 0)).toBe(0);
  });
});

describe('max-normalisation', () => {
  it('reproduces the artboard’s own ratio', () => {
    expect(shareOfMax(3140, 4021)).toBeCloseTo(0.78, 2);
  });

  it('draws an empty track when nothing has happened yet', () => {
    expect(shareOfMax(0, 0)).toBe(0);
    expect(shareOfMax(5, 0)).toBe(0);
  });
});

describe('the query-type fold', () => {
  const perType = { A: 100, AAAA: 50, HTTPS: 20, PTR: 10, NS: 4, SOA: 3, MX: 1 };

  it('keeps the four largest and folds the rest into one `other`', () => {
    const slices = queryTypeSlices(perType);
    expect(slices.map((s) => s.label)).toEqual([
      'A',
      'AAAA',
      'HTTPS',
      'PTR',
      OTHER_LABEL,
    ]);
    expect(slices[4]?.value).toBe(8);
  });

  it('never renames a single leftover label to `other`', () => {
    const slices = queryTypeSlices({ A: 5, AAAA: 4, HTTPS: 3, PTR: 2, NS: 1 });
    expect(slices.map((s) => s.label)).toEqual([
      'A',
      'AAAA',
      'HTTPS',
      'PTR',
      'NS',
    ]);
  });

  it('drops zero labels rather than drawing a zero-width slice', () => {
    expect(queryTypeSlices({ A: 5, MX: 0 }).map((s) => s.label)).toEqual(['A']);
  });

  it('is empty for an empty range', () => {
    expect(queryTypeSlices({})).toEqual([]);
  });

  it('sums per_type across the range, absent labels counting as zero', () => {
    expect(
      sumPerType([
        { per_type: { A: 3, AAAA: 1 } },
        { per_type: { A: 2, HTTPS: 4 } },
      ]),
    ).toEqual({ A: 5, AAAA: 1, HTTPS: 4 });
  });
});

describe('durations and timestamps', () => {
  it('prints uptime as the artboard draws it', () => {
    expect(formatUptime(4 * 3600 + 31 * 60)).toBe('4h 31m');
    expect(formatUptime(90)).toBe('1m');
    expect(formatUptime(12)).toBe('12s');
    expect(formatUptime(3 * 86400 + 4 * 3600)).toBe('3d 4h');
  });

  it('says `never` for a list not yet refreshed in this process', () => {
    expect(lastRefreshLabel(null, Date.now())).toBe('never');
  });

  it('names yesterday, and dates anything older', () => {
    const now = new Date(2026, 7, 20, 12, 0, 0);
    const today = new Date(2026, 7, 20, 4, 0, 0);
    const yesterday = new Date(2026, 7, 19, 4, 0, 0);
    const older = new Date(2026, 7, 12, 4, 0, 0);
    expect(lastRefreshLabel(today.toISOString(), now.getTime())).toBe('04:00');
    expect(lastRefreshLabel(yesterday.toISOString(), now.getTime())).toBe(
      'yesterday 04:00',
    );
    expect(lastRefreshLabel(older.toISOString(), now.getTime())).toContain(
      '04:00',
    );
    expect(lastRefreshLabel(older.toISOString(), now.getTime())).not.toContain(
      'yesterday',
    );
  });
});

describe('the upstream bar (D4, option A)', () => {
  it('scales the bar by workload against the busiest endpoint', () => {
    const max = 201883;
    expect(upstreamBar(201883, 12, max).width).toBe(1);
    expect(upstreamBar(61402, 4, max).width).toBeCloseTo(0.304, 3);
    expect(upstreamBar(8204, 311, max).width).toBeCloseTo(0.0406, 4);
  });

  it('keeps the artboard’s ranking — the busiest endpoint draws the longest bar', () => {
    const max = 201883;
    const widths = [201883, 61402, 8204].map(
      (attempts) => upstreamBar(attempts, 0, max).width,
    );
    expect(widths[0]).toBeGreaterThan(widths[1]!);
    expect(widths[1]).toBeGreaterThan(widths[2]!);
  });

  it('overlays the failure rate within the bar, not within the track', () => {
    expect(upstreamBar(8204, 311, 201883).overlay).toBeCloseTo(0.0379, 4);
  });

  it('fills the bar when every attempt failed', () => {
    expect(upstreamBar(50, 50, 100)).toEqual({ width: 0.5, overlay: 1 });
  });

  it('draws an empty track for an endpoint nothing has been asked of', () => {
    expect(upstreamBar(0, 0, 201883)).toEqual({ width: 0, overlay: 0 });
  });

  it('draws every row empty before any query has been sent', () => {
    expect(upstreamBar(0, 0, 0)).toEqual({ width: 0, overlay: 0 });
  });

  it('gives a sub-pixel overlay no minimum width at all', () => {
    const { width, overlay } = upstreamBar(201883, 12, 201883);
    // 600 px of track: the band is well under one pixel and stays there.
    expect(width * overlay * 600).toBeLessThan(1);
    expect(overlay).toBeGreaterThan(0);
  });

  it('cannot exceed the bar even if a response were inconsistent', () => {
    expect(upstreamBar(10, 99, 10).overlay).toBe(1);
  });
});
