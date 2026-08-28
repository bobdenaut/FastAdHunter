import { describe, expect, it } from 'vitest';
import {
  OTHER_LABEL,
  blockedPercent,
  extentOf,
  faultRate,
  listsNeedingAttention,
  stackedMemory,
  statsBytes,
  upstreamStateCounts,
  budgetProximity,
  cacheHitRate,
  cacheLookups,
  closestBound,
  failureRunShares,
  freeEntries,
  latencyMs,
  latestLatency,
  passDelta,
  qpsStats,
  queryTypeSlices,
  shareOfMax,
  sliceShare,
  sumOver,
  sumPerType,
  upstreamBar,
  upstreamMode,
  windowTrend,
} from './derive';
import {
  compactCount,
  epochSeconds,
  formatMiB,
  latencyMsLabel,
  microsLabel,
  millisLabel,
  msAxisLabel,
  percent1,
  qpsLabel,
} from './charts/format';
import { formatUptime, lastRefreshLabel } from './time';
import {
  WATCH_THRESHOLD,
  latestRssState,
  rssStates,
} from './pages/memory/budgets';

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

  it('gives each slice its share of the summed total', () => {
    expect(sliceShare(114224, 184233)).toBeCloseTo(62.0, 1);
  });

  it('is zero on an empty range rather than NaN', () => {
    // An all-zero donut draws its empty track; five NaNs would render as
    // `NaN%` beside five zeroes.
    expect(sliceShare(0, 0)).toBe(0);
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

/* ------------------------------------------------------- p5-08 runtime pages */

describe('the cache figures', () => {
  it('never draws a negative free band', () => {
    // `capacity` is per-shard × shard count and can round below the configured
    // maximum, so `entries` above it is a real reading rather than a bug.
    expect(freeEntries(50_000, 1_108)).toBe(48_892);
    expect(freeEntries(10_000, 10_400)).toBe(0);
  });

  it('counts lookups as hits plus misses, which is resolved queries', () => {
    expect(cacheLookups(10_021, 1_150)).toBe(11_171);
  });

  it('gives an untouched cache a zero hit rate rather than a NaN', () => {
    expect(cacheHitRate(0, 0)).toBe(0);
    expect(cacheHitRate(10_021, 1_150)).toBeCloseTo(89.7, 1);
  });

  it('names whichever bound is higher, and says so when they are level', () => {
    expect(closestBound(2.2, 1.7)).toBe('entries');
    expect(closestBound(1.7, 2.2)).toBe('bytes');
    expect(closestBound(2.2, 2.2)).toBe('equal');
  });

  it('prints bytes in the artboard’s own MiB form', () => {
    expect(formatMiB(67_108_864)).toBe('64 MiB');
    expect(formatMiB(2_846_720)).toBe('2.7 MiB');
    expect(formatMiB(9_871_232)).toBe('9.4 MiB');
  });

  it('prints a sweep duration as the artboard does, both figures', () => {
    expect(millisLabel(1842 / 1000)).toBe('1.84');
    expect(millisLabel(4.7)).toBe('4.7');
  });
});

describe('latency, seconds to milliseconds', () => {
  it('maps an exact 0.0 to a gap, because it means no traffic', () => {
    // `LatencySummary` reports 0.0 for a stage with no queries in the interval.
    // A real reading is a bucket upper bound and can never be exactly zero, so
    // there is no measurement to lose here.
    expect(latencyMs(0)).toBeNull();
  });

  it('converts a real reading', () => {
    expect(latencyMs(0.000_039)).toBeCloseTo(0.039, 6);
    expect(latencyMs(0.000_412)).toBeCloseTo(0.412, 6);
  });

  it('formats a tile figure at the artboard’s precision', () => {
    expect(latencyMsLabel(0.039)).toBe('0.039');
    expect(latencyMsLabel(0.412)).toBe('0.412');
  });

  it('labels the axis the way the artboard draws it', () => {
    expect(msAxisLabel(1)).toBe('1.0');
    expect(msAxisLabel(0.75)).toBe('0.75');
    expect(msAxisLabel(0.5)).toBe('0.50');
    expect(msAxisLabel(0.25)).toBe('0.25');
  });
});

describe('the budget proximity bar', () => {
  it('fills proportionally while under the budget, and is not over', () => {
    const bar = budgetProximity(0.412, 1);
    expect(bar.percent).toBeCloseTo(41.2, 6);
    expect(bar.over).toBe(false);
    expect(budgetProximity(0.039, 1).over).toBe(false);
  });

  it('flips at the budget itself — the one documented boundary', () => {
    expect(budgetProximity(1, 1)).toEqual({ percent: 100, over: true });
  });

  it('caps the bar rather than painting past its track', () => {
    expect(budgetProximity(3.4, 1)).toEqual({ percent: 100, over: true });
  });
});

describe('the QPS stat row', () => {
  it('takes the latest and the busiest of the served rows', () => {
    expect(qpsStats([10, 28.4, 12.5])).toEqual({ latest: 12.5, busiest: 28.4 });
  });

  it('skips rows the `fields` trim dropped rather than reading them as zero', () => {
    expect(qpsStats([10, undefined, 4])).toEqual({ latest: 4, busiest: 10 });
  });

  it('has nothing to report on an empty range', () => {
    expect(qpsStats([])).toEqual({ latest: null, busiest: null });
  });
});

describe('the pass band', () => {
  it('is queries minus blocked minus the real allow verdict', () => {
    expect(passDelta(750, 210, 5)).toBe(535);
  });

  it('floors at zero across a restart boundary', () => {
    // The server deltas against its own previous snapshot; a restart inside the
    // range zeroes the cumulative counters and one sample comes back with the
    // parts exceeding the whole.
    expect(passDelta(0, 210, 5)).toBe(0);
  });
});

describe('the failure-run histogram', () => {
  it('normalises to the row’s own largest bucket', () => {
    expect(failureRunShares([12, 21, 30, 39])).toEqual([
      12 / 39,
      21 / 39,
      30 / 39,
      1,
    ]);
  });

  it('draws four empty tracks when the endpoint has closed no runs', () => {
    expect(failureRunShares([0, 0, 0, 0])).toEqual([0, 0, 0, 0]);
  });
});

describe('the upstream rendering mode', () => {
  it('is the strategy when the configuration named one', () => {
    expect(upstreamMode('adaptive')).toBe('adaptive');
    expect(upstreamMode('fallback')).toBe('fallback');
  });

  it('is unknown when `/config` could not be read', () => {
    // Not "adaptive by default": under `fallback` the zeros mean no health
    // state exists, so guessing states the opposite of the truth.
    expect(upstreamMode(null)).toBe('unknown');
    expect(upstreamMode(undefined)).toBe('unknown');
    expect(upstreamMode('something-new')).toBe('unknown');
  });
});

describe('the two unit conversions that are formatting, not derivation', () => {
  it('turns the cleanup gauge’s microseconds into the artboard’s figure', () => {
    expect(microsLabel(1842)).toBe('1.84');
    expect(microsLabel(10)).toBe('0.01');
  });

  it('turns an RFC 3339 stamp into the epoch seconds uPlot takes', () => {
    expect(epochSeconds('2026-08-27T10:00:00Z')).toBe(
      Date.UTC(2026, 7, 27, 10, 0, 0) / 1000,
    );
  });
});

describe('E10 — which row a latency tile reads', () => {
  function row(p99: number | null, ts = '2026-08-27T10:00:00Z') {
    return p99 === null
      ? { ts }
      : {
          ts,
          latency: {
            block_p50: 0,
            block_p99: p99,
            cache_hit_p50: 0,
            cache_hit_p99: 0,
            forward_p50: 0,
            forward_p99: 0,
          },
        };
  }

  it('takes the last served row, not the first', () => {
    expect(latestLatency([row(0.000_039), row(0.000_077)])?.block_p99).toBe(
      0.000_077,
    );
  });

  it('skips a row whose `latency` key was trimmed away', () => {
    // `fields` drops a key entirely — absent, not null — and an absent key is
    // not a measurement of nothing.
    expect(latestLatency([row(0.000_039), row(null)])?.block_p99).toBe(
      0.000_039,
    );
  });

  it('answers null when no served row carries one', () => {
    expect(latestLatency([])).toBeNull();
    expect(latestLatency([row(null), row(null)])).toBeNull();
  });
});

describe('qpsLabel', () => {
  it('prints the stat row’s one decimal', () => {
    expect(qpsLabel(12.5)).toBe('12.5');
    expect(qpsLabel(28.4)).toBe('28.4');
  });

  it('keeps the decimal on a whole figure, as the artboard draws it', () => {
    expect(qpsLabel(20)).toBe('20.0');
    expect(qpsLabel(0)).toBe('0.0');
  });
});

describe('the Diagnostics derivations', () => {
  it('counts the three upstream states and no fourth (D3)', () => {
    const rows = [
      { state: 'healthy' as const },
      { state: 'penalized' as const },
      { state: 'probing' as const },
      { state: 'probing' as const },
    ];
    expect(upstreamStateCounts(rows)).toEqual({
      healthy: 1,
      penalized: 1,
      probing: 2,
    });
    // The counts always sum to the rows, which the artboard's own figures do
    // not.
    const counts = upstreamStateCounts(rows);
    expect(counts.healthy + counts.penalized + counts.probing).toBe(rows.length);
  });

  it('ignores a state outside the vocabulary rather than counting NaN', () => {
    // `counts[unmodelled] += 1` wrote `NaN` **and** added the key. Invisible on
    // screen, because the rendered line reads only the three known states —
    // which is why the "invents no fourth state" test passed over it.
    const counts = upstreamStateCounts([
      { state: 'healthy' as const },
      { state: 'recovering' } as unknown as { state: 'healthy' },
    ]);
    expect(counts).toEqual({ healthy: 1, penalized: 0, probing: 0 });
    expect(Object.keys(counts)).toEqual(['healthy', 'penalized', 'probing']);
    for (const value of Object.values(counts)) {
      expect(Number.isNaN(value)).toBe(false);
    }
  });

  it('counts only failed and rejected lists as needing attention (D4)', () => {
    const items = [
      { last_status: 'ok' as const },
      { last_status: 'degraded' as const },
      { last_status: 'failed' as const },
      { last_status: 'rejected' as const },
      { last_status: 'never' as const },
    ];
    expect(listsNeedingAttention(items).map((item) => item.last_status)).toEqual(
      ['failed', 'rejected'],
    );
  });

  it('sums the two stats structures into one slice (D5)', () => {
    expect(
      statsBytes({ stats_aggregates_bytes: 41_984, stats_clients_bytes: 9_216 }),
    ).toBe(51_200);
  });


  it('reads the window extremes and skips absent rows (D7 / D11)', () => {
    expect(
      extentOf([{ v: 4 }, { v: undefined }, { v: 9 }, { v: 2 }], (item) => item.v),
    ).toEqual({ min: 2, max: 9 });
    expect(extentOf([], (item: { v: number }) => item.v)).toBeNull();
  });

  it('divides the fault delta by the elapsed seconds (D12)', () => {
    const rate = faultRate(
      { ts: '2026-08-01T00:00:00Z', minor_page_faults: 1_000 },
      { ts: '2026-08-01T00:01:00Z', minor_page_faults: 8_080 },
    );
    expect(rate).toBe(118);
  });

  it('answers null for a pair that spans a restart (D12)', () => {
    // A cumulative counter that fell is a new process, never a negative rate.
    expect(
      faultRate(
        { ts: '2026-08-01T00:00:00Z', minor_page_faults: 8_000 },
        { ts: '2026-08-01T00:01:00Z', minor_page_faults: 12 },
      ),
    ).toBeNull();
    expect(
      faultRate(
        { ts: '2026-08-01T00:00:00Z' },
        { ts: '2026-08-01T00:01:00Z', minor_page_faults: 12 },
      ),
    ).toBeNull();
  });

  it('stacks the bands so the top edge is `rss_bytes` itself (D13)', () => {
    const item = {
      rss_bytes: 100,
      memory: {
        ruleset_bytes: 40,
        cache_estimated_bytes: 10,
        stats_aggregates_bytes: 3,
        stats_clients_bytes: 2,
      },
    };
    const bands = stackedMemory([item]);
    expect(bands.map((band) => band[0])).toEqual([40, 50, 55, 100]);
    // The identity is the server's: `accounted + residual = rss`. The chart
    // renders it rather than re-deriving the residual.
    expect(bands[3]?.[0]).toBe(item.rss_bytes);
  });

  it('gaps the components of an over-accounted row rather than inverting the stack', () => {
    // `fah-model`'s `over_accounted()` state: components claim more than RSS,
    // which is an accounting bug and never a reading. Stacked, the top band
    // falls below the one under it, which reads as a component shrinking.
    expect(
      stackedMemory([
        {
          rss_bytes: 50,
          memory: {
            ruleset_bytes: 40,
            cache_estimated_bytes: 10,
            stats_aggregates_bytes: 8,
            stats_clients_bytes: 2,
          },
        },
      ]),
    ).toEqual([[null], [null], [null], [50]]);
  });

  it('keeps the RSS of an over-accounted row, which is not the figure in doubt', () => {
    // RSS comes from `/proc/self/status`; it is what the components failed to
    // add up to. It is also the series the page's RSS state is walked from, and
    // the KPI card walks the same readings off the row itself — a gap in one
    // and not the other is the card and the line disagreeing about a reading.
    const bands = stackedMemory([
      {
        rss_bytes: 90,
        memory: {
          ruleset_bytes: 40,
          cache_estimated_bytes: 10,
          stats_aggregates_bytes: 8,
          stats_clients_bytes: 2,
        },
      },
      {
        rss_bytes: 50,
        memory: {
          ruleset_bytes: 40,
          cache_estimated_bytes: 10,
          stats_aggregates_bytes: 8,
          stats_clients_bytes: 2,
        },
      },
    ]);
    expect(bands[3]).toEqual([90, 50]);
    expect(bands[2]).toEqual([60, null]);
  });

  /** A memory block whose four components sum to exactly `total`. */
  function parts(total: number): {
    ruleset_bytes: number;
    cache_estimated_bytes: number;
    stats_aggregates_bytes: number;
    stats_clients_bytes: number;
  } {
    const share = Math.floor(total / 4);
    return {
      ruleset_bytes: total - 3 * share,
      cache_estimated_bytes: share,
      stats_aggregates_bytes: share,
      stats_clients_bytes: share,
    };
  }

  /** A well-accounted row: the components take `accounted` of `rss`. */
  function row(rss: number, accountedShare: number) {
    return { rss_bytes: rss, memory: parts(Math.floor(rss / accountedShare)) };
  }

  it('walks the same RSS readings the KPI card walks — U1, over an over-accounted row', () => {
    // U1's invariant: the chart's line and the card's figure read one state
    // walk, so they cannot disagree about a reading. The chart walks the stack's
    // top band and the card walks the rows, so the two series have to hold the
    // same values — which is why an over-accounted row keeps its RSS.
    const rows = [
      row(WATCH_THRESHOLD + 4_000_000, 10),
      // Over-accounted: the components claim more than RSS.
      { rss_bytes: WATCH_THRESHOLD - 1_000_000, memory: parts(WATCH_THRESHOLD) },
      row(WATCH_THRESHOLD - 1_000_000, 10),
    ];
    const top = stackedMemory(rows)[3] as (number | null)[];
    const fromRows = rows.map((item) => item.rss_bytes);
    expect(top).toEqual(fromRows);
    expect(rssStates(top)).toEqual(rssStates(fromRows));
    // And the hysteresis still carries: 1 MiB under the watch point is inside
    // the 3 MiB band, so a reading that arrived from `watch` stays there.
    expect(rssStates(top)).toEqual(['watch', 'watch', 'watch']);
    expect(latestRssState(top)).toBe('watch');
  });

  it('carries the state across a row that has no reading at all', () => {
    // A gap is a row nobody wrote, not evidence that RSS fell — the sample
    // after it is judged against the state before it.
    const values = [WATCH_THRESHOLD + 1, null, WATCH_THRESHOLD - 1_000_000];
    expect(rssStates(values)).toEqual(['watch', null, 'watch']);
    expect(latestRssState(values)).toBe('watch');
    // Without the carry the third reading is plainly under the threshold.
    expect(rssStates([values[2] as number])).toEqual(['normal']);
  });

  it('keeps a row whose components sum to exactly RSS', () => {
    // The boundary the gap must not swallow: equality is the identity holding,
    // not the bug.
    const bands = stackedMemory([
      {
        rss_bytes: 55,
        memory: {
          ruleset_bytes: 40,
          cache_estimated_bytes: 10,
          stats_aggregates_bytes: 3,
          stats_clients_bytes: 2,
        },
      },
    ]);
    expect(bands.map((band) => band[0])).toEqual([40, 50, 55, 55]);
  });

  it('draws a gap rather than a zero column for a row with no memory (D13)', () => {
    expect(stackedMemory([{ rss_bytes: 100 }])).toEqual([
      [null],
      [null],
      [null],
      [null],
    ]);
  });

});

describe('windowTrend (D14)', () => {
  const flat = [100, 102, 99, 101, 100, 103];

  it('answers null, not flat, on a window too short to have a shape', () => {
    // The distinction the verdict pill and the fault rate both depend on:
    // "not enough history" is not the same claim as "steady".
    expect(windowTrend([100, 100, 100, 100, 100], 0.1)).toBeNull();
    expect(windowTrend(flat, 0.1)).toBe('flat');
  });

  it('holds flat through jitter inside the tolerance', () => {
    expect(windowTrend(flat, 0.1)).toBe('flat');
  });

  it('reads a climb past the tolerance as rising', () => {
    expect(windowTrend([100, 100, 100, 130, 130, 130], 0.1)).toBe('rising');
    // …and the same climb is flat under a tolerance wide enough to cover it.
    expect(windowTrend([100, 100, 100, 130, 130, 130], 0.5)).toBe('flat');
  });

  it('reads a fall past the tolerance as falling', () => {
    expect(windowTrend([130, 130, 130, 100, 100, 100], 0.1)).toBe('falling');
  });
});
