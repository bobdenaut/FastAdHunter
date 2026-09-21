// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DebugMemory, HistoryPerf, PerfItem } from '../api/types';
import type { Route } from '../router/routes';
import DiagnosticsMemory from './diagnostics-memory';
import { AllocatorCard } from './memory/allocator-card';
import { CompositionCard } from './memory/composition-card';
import { FaultsCard } from './memory/faults-card';
import { KpiRail } from './memory/kpi-rail';
import { PeakSpark, Sparkline } from './memory/sparkline';
import { formatMiB } from '../charts/format';
import { formatUptime } from '../time';
import {
  CEILING_BUDGET,
  budgetLabel,
  HYSTERESIS,
  STEADY_STATE_BUDGET,
  WATCH_THRESHOLD,
  rssState,
  rssStates,
} from './memory/budgets';

/**
 * The three claims this page must not make: that the budgets are enforced, that
 * peak is part of current RSS, and that the window maximum is an all-time one.
 */

const ROUTE: Route = {
  path: '/diagnostics/memory',
  title: 'Memory',
  section: 'system',
  group: 'diagnostics',
  events: [],
  endpoints: [],
  built: true,
  ownsHeader: true,
  load: null,
};

const MEMORY: DebugMemory = {
  ruleset_bytes: 25_165_824,
  cache_estimated_bytes: 1_153_434,
  stats_aggregates_bytes: 1_048_576,
  stats_clients_bytes: 629_145,
  accounted_bytes: 27_996_979,
  residual_bytes: 29_570_662,
  cache_entries: 1_108,
  process_rss: 57_567_641,
  process_peak_rss: 140_194_611,
  major_page_faults: 0,
  minor_page_faults: 4_211_337,
  process_rss_anon: 40_000_000,
  process_rss_file: 17_567_641,
  allocator_committed_bytes: 333_447_168,
  allocator_committed_peak_bytes: 333_447_168,
};

function perfRow(over: Record<string, unknown> = {}) {
  return {
    ts: '2026-08-28T00:00:00Z',
    rss_bytes: 57_567_641,
    peak_rss: 140_194_611,
    memory: {
      ruleset_bytes: 25_165_824,
      cache_estimated_bytes: 1_153_434,
      stats_aggregates_bytes: 1_048_576,
      stats_clients_bytes: 629_145,
      accounted_bytes: 27_996_979,
      residual_bytes: 29_570_662,
    },
    minor_page_faults: 4_211_337,
    ...over,
  };
}

const TELEMETRY = {
  process: { version: '0.2.20', uptime_seconds: 16_260 },
  ruleset: { rules: 752_585, duplicates_removed: 0, compile_duration_seconds: 1 },
};

const HISTORY = {
  from: '2026-08-27T00:00:00Z',
  to: '2026-08-28T00:00:00Z',
  stride: 1,
  items: [
    perfRow(),
    perfRow({ ts: '2026-08-28T00:01:00Z', minor_page_faults: 4_218_417 }),
  ],
};

function respond(status: number, body?: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: () => null },
    json: async () => {
      if (body === undefined) throw new Error('no body');
      return body;
    },
  } as unknown as Response;
}

let host: HTMLElement | null = null;
let requests: string[] = [];

function mount(node: preact.ComponentChild): HTMLElement {
  host = document.createElement('div');
  document.body.append(host);
  act(() => {
    render(node, host as HTMLElement);
  });
  return host;
}

async function flush(): Promise<void> {
  await act(async () => {
    for (let turn = 0; turn < 12; turn += 1) await Promise.resolve();
  });
}

async function mountPage(
  memory: unknown = MEMORY,
  history: unknown = HISTORY,
): Promise<HTMLElement> {
  requests = [];
  const fetchMock = vi.fn((url: string) => {
    requests.push(url);
    if (url === '/api/v1/debug/memory') {
      return Promise.resolve(respond(200, memory));
    }
    if (url.startsWith('/api/v1/telemetry')) {
      return Promise.resolve(respond(200, TELEMETRY));
    }
    if (url.startsWith('/api/v1/history/perf')) {
      return Promise.resolve(respond(200, history));
    }
    throw new Error(`unexpected ${url}`);
  });
  vi.stubGlobal('fetch', fetchMock);
  const dom = mount(<DiagnosticsMemory route={ROUTE} />);
  await flush();
  return dom;
}

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn());
});

afterEach(() => {
  if (host !== null) {
    act(() => {
      render(null, host as HTMLElement);
    });
    host.remove();
    host = null;
  }
  vi.unstubAllGlobals();
});
describe('the composition bar', () => {
  it('draws exactly the four segments that sum to RSS', () => {
    const dom = mount(<CompositionCard memory={MEMORY} />);
    const segments = dom.querySelectorAll('.composition-seg');
    expect(segments).toHaveLength(4);

    // The widths are shares of RSS, so they sum to 100 — the card's whole
    // claim, asserted rather than eyeballed.
    const total = [...segments].reduce((sum, node) => {
      const width = (node as HTMLElement).style.width;
      return sum + Number.parseFloat(width);
    }, 0);
    expect(total).toBeCloseTo(100, 1);
  });

  it('never puts the peak among them', () => {
    const dom = mount(<CompositionCard memory={MEMORY} />);
    // 133.7 MiB is the fixture's peak. A lifetime high-water mark is not part
    // of current RSS, and a segment for it would draw a whole that does not
    // exist.
    expect(dom.textContent).not.toContain('133.7');
    expect(dom.querySelectorAll('.composition-tile')).toHaveLength(4);
  });

  it('renders the residual as a texture rather than a hue', () => {
    const dom = mount(<CompositionCard memory={MEMORY} />);
    expect(dom.querySelector('.seg-residual')).not.toBeNull();
    expect(dom.querySelector('.sw-residual')).not.toBeNull();
  });

  it('prints the compiled rule count beside the ruleset share', () => {
    // The figure is already on screen in Key metrics, from the same
    // `/telemetry` read — the tile said `compiled rules` and dropped it.
    const dom = mount(
      <CompositionCard memory={MEMORY} rules={TELEMETRY.ruleset.rules} />,
    );
    const tile = [...dom.querySelectorAll('.composition-tile')].find((node) =>
      (node.textContent ?? '').includes('Ruleset'),
    );
    expect(tile?.textContent).toContain(
      `${TELEMETRY.ruleset.rules.toLocaleString()} rules`,
    );
  });

  it('falls back to the wording when telemetry did not answer', () => {
    const dom = mount(<CompositionCard memory={MEMORY} rules={null} />);
    const tile = [...dom.querySelectorAll('.composition-tile')].find((node) =>
      (node.textContent ?? '').includes('Ruleset'),
    );
    expect(tile?.textContent).toContain('compiled rules');
  });

  it('stops claiming the components sum to RSS when they exceed it', () => {
    // Residual is a server-side `saturating_sub`, so an over-accounted reading
    // arrives as residual 0 beside an `accounted` above RSS. The card stated
    // the identity unconditionally, which is the one thing it must not do in
    // the state that identity fails in.
    const dom = mount(
      <CompositionCard
        memory={{
          ...MEMORY,
          accounted_bytes: 60_000_000,
          residual_bytes: 0,
          process_rss: 57_567_641,
        }}
      />,
    );
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('claim more than RSS');
    expect(text).not.toContain('sum to RSS exactly');
  });

  it('keeps the claim when the identity holds', () => {
    const dom = mount(<CompositionCard memory={MEMORY} />);
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('sum to RSS exactly');
    expect(text).not.toContain('claim more than RSS');
  });

  it('renders the unavailable state rather than zeros off Linux', () => {
    const dom = mount(
      <CompositionCard
        memory={{ ...MEMORY, process_rss: null, residual_bytes: null }}
      />,
    );
    expect(dom.textContent).toContain('RSS unavailable on this platform');
    expect(dom.querySelector('.composition-seg')).toBeNull();
  });
});

describe('the plot options', () => {
  it('places every series in the slot its readers use', async () => {
    // `restartsOf` and `stateStroke` read `u.data` positionally. The series
    // array is assembled from the same constant, so a reordering cannot leave
    // restart detection pointed at the wrong line in silence.
    const { memoryTrendOptions } = await import('./memory/trend-options');
    const { readChartTheme } = await import('../charts/theme');
    const options = memoryTrendOptions({
      theme: readChartTheme(),
      range: '24h',
      onCursor: () => undefined,
      onPlace: () => undefined,
    });
    expect(options.series?.map((series) => series.label)).toEqual([
      undefined,
      'ruleset',
      'cache',
      'stats',
      'RSS',
      'peak',
      'faults',
    ]);
  });
});

describe('the figures that come from a constant', () => {
  it('prints uptime the way every other page does', async () => {
    const dom = await mountPage();
    const row = [...dom.querySelectorAll('tr')].find((node) =>
      (node.textContent ?? '').includes('uptime'),
    );
    // One formatter, so the same reading cannot read `4 h 31 m` here and
    // `4h 31m` on Health.
    expect(row?.textContent).toContain(
      formatUptime(TELEMETRY.process.uptime_seconds),
    );
    expect(formatUptime(16_260)).toBe('4h 31m');
  });

  it('derives both budget figures from the budget itself', async () => {
    expect(budgetLabel(STEADY_STATE_BUDGET)).toBe('128 MB');
    expect(budgetLabel(CEILING_BUDGET)).toBe('256 MB');
    // The rail's right end and the peak card's tick caption are the two the
    // literal used to be written into.
    const dom = await mountPage();
    const rails = [...dom.querySelectorAll('.railx')].map(
      (node) => node.textContent ?? '',
    );
    expect(
      rails.some((text) => text.endsWith(budgetLabel(STEADY_STATE_BUDGET))),
    ).toBe(true);
    expect(
      rails.some((text) =>
        text.includes(`${budgetLabel(STEADY_STATE_BUDGET)} budget`),
      ),
    ).toBe(true);
  });

  it('formats the two threshold captions from the thresholds', () => {
    // The chart writes these onto a canvas, which a jsdom test cannot read —
    // so the derivation is asserted instead: these are the exact strings the
    // sketch captions carry, and a budget change moves both.
    expect(formatMiB(WATCH_THRESHOLD)).toBe('100 MiB');
    expect(formatMiB(STEADY_STATE_BUDGET)).toBe('122.1 MiB');
  });
});

describe('the KPI sparklines', () => {
  /** A window of readings as `PerfItem`s, for the sparks that walk history. */
  function window(rss: number[]): PerfItem[] {
    return rss.map((rss_bytes) => perfRow({ rss_bytes }) as PerfItem);
  }

  it('fills the area under the RSS line with a gradient, as the sketch draws it', () => {
    const dom = mount(
      <Sparkline
        items={window([40, 44, 41, 47])}
        of={(item) => item.rss_bytes ?? null}
        tone="ink"
      />,
    );
    const area = dom.querySelector('.spark-area');
    expect(area).not.toBeNull();
    expect(area?.getAttribute('fill')).toBe('url(#kpi-spark-fill-ink)');
    expect(dom.querySelector('linearGradient')?.id).toBe('kpi-spark-fill-ink');
    // The fill closes to the box floor; the line does not.
    expect(area?.getAttribute('d')).toContain(',38.0');
    expect(dom.querySelector('.spark-line')?.getAttribute('d')).not.toContain(
      ',38.0',
    );
  });

  it('breaks the fill where the record has a gap, never bridging it', () => {
    // `0` stands in for the absent row here; the accessor is what decides a
    // reading is missing, exactly as the page's own accessors do.
    const dom = mount(
      <Sparkline
        items={window([40, 44, 0, 41, 47, 45])}
        of={(item) => (item.rss_bytes === 0 ? null : (item.rss_bytes ?? null))}
        tone="residual"
      />,
    );
    // Two runs, so two closed sub-paths: a gap in the record is a gap in the
    // shape, not a straight line drawn through a measurement nobody took.
    const area = dom.querySelector('.spark-area')?.getAttribute('d') ?? '';
    expect(area.match(/Z/g)).toHaveLength(2);
  });

  it('draws peak as a step and marks every drop', () => {
    const items = [
      perfRow({ peak_rss: 140_000_000 }),
      perfRow({ ts: '2026-08-28T00:01:00Z', peak_rss: 140_000_000 }),
      // A fall in this series is always a restart, never a reclaim.
      perfRow({ ts: '2026-08-28T00:02:00Z', peak_rss: 90_000_000 }),
      perfRow({ ts: '2026-08-28T00:03:00Z', peak_rss: 90_000_000 }),
    ] as PerfItem[];
    const dom = mount(<PeakSpark items={items} />);
    const path = dom.querySelector('.spark-line')?.getAttribute('d') ?? '';
    // Step-after: the level is held to the next x before it moves vertically,
    // so no segment slopes. Every pair of consecutive commands shares either
    // its x or its y.
    const points = [...path.matchAll(/[ML](-?[\d.]+),(-?[\d.]+)/g)].map((at) => [
      Number(at[1]),
      Number(at[2]),
    ]);
    expect(points.length).toBeGreaterThan(2);
    for (let index = 1; index < points.length; index += 1) {
      const [x0, y0] = points[index - 1] as number[];
      const [x1, y1] = points[index] as number[];
      expect(x0 === x1 || y0 === y1).toBe(true);
    }
    expect(dom.querySelectorAll('.spark-drop')).toHaveLength(1);
    // No area under a high-water mark: the band below it is not a quantity.
    expect(dom.querySelector('.spark-area')).toBeNull();
  });

  it('marks no drop on a peak that only holds', () => {
    const items = window([1, 2, 3, 4]).map(
      (item, index) =>
        ({ ...item, peak_rss: 140_000_000 + index }) as PerfItem,
    );
    const dom = mount(<PeakSpark items={items} />);
    expect(dom.querySelectorAll('.spark-drop')).toHaveLength(0);
  });
});

describe('the fault rate card', () => {
  /** `count` samples a minute apart, each adding `delta` faults. */
  function ramp(deltas: number[]): HistoryPerf {
    let faults = 1_000_000;
    const items = deltas.map((delta, index) => {
      faults += delta;
      return perfRow({
        ts: new Date(Date.UTC(2026, 7, 28, 0, index)).toISOString(),
        minor_page_faults: faults,
      });
    });
    return { ...HISTORY, items: items as PerfItem[] };
  }

  it('labels the window ends, since the plot draws no x ticks', () => {
    const dom = mount(
      <FaultsCard history={ramp(Array(8).fill(7_080))} range="24h" />,
    );
    const ends = dom.querySelector('.faults-xends');
    expect(ends?.textContent).toContain('24 h ago');
    expect(ends?.textContent).toContain('now');
  });

  it('names the shape of the rate beside the figure', () => {
    const dom = mount(
      <FaultsCard history={ramp(Array(8).fill(7_080))} range="24h" />,
    );
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('118 /s · flat');
  });

  it('says rising when the rate actually climbs', () => {
    const dom = mount(
      <FaultsCard
        history={ramp([7_080, 7_080, 7_080, 7_080, 30_000, 30_000, 30_000, 30_000])}
        range="24h"
      />,
    );
    expect((dom.textContent ?? '').replace(/\s+/g, ' ')).toContain('· rising');
  });

  it('claims no shape on a window too short to have one', () => {
    // Two samples make one rate. A descriptor there would be a reading of a
    // trend nobody observed.
    const dom = mount(<FaultsCard history={ramp([7_080, 7_080])} range="24h" />);
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('118 /s');
    expect(text).not.toContain('· flat');
    expect(dom.querySelector('.faults-xends')).toBeNull();
  });
});

describe('the allocator table', () => {
  it('tags both allocator rows as carrying no contract', () => {
    const dom = mount(<AllocatorCard memory={MEMORY} history={null} range="24h" />);
    const tags = [...dom.querySelectorAll('.tagq')].map(
      (node) => node.textContent,
    );
    expect(tags).toEqual(['no contract', 'no contract']);
  });

  it('prints the lifetime fault counter in full, never compacted', () => {
    const dom = mount(<AllocatorCard memory={MEMORY} history={null} range="24h" />);
    const row = [...dom.querySelectorAll('tr')].find((node) =>
      (node.textContent ?? '').includes('minor page faults'),
    );
    // The row is labelled `lifetime` and the rate is charted next door, so the
    // exact counter is the only thing this row is for. `4.2M` is the rate
    // card's job done badly.
    expect(row?.textContent).toContain((4_211_337).toLocaleString());
    expect(row?.textContent).not.toContain('4.2M');
  });

  it('renders a null allocator reading as unavailable, never as zero', () => {
    const dom = mount(
      <AllocatorCard
        memory={{
          ...MEMORY,
          allocator_committed_bytes: null,
          allocator_committed_peak_bytes: null,
        }}
        history={null}
        range="24h"
      />,
    );
    // `null` is "the allocator reported nothing". A zero would be a
    // measurement nobody took, and it would read as an allocator holding no
    // memory at all.
    expect(dom.textContent).not.toContain('0 MiB');
    expect(dom.textContent).toContain('—');
  });

  it('keeps the kernel readings out of the no-contract class', () => {
    const dom = mount(<AllocatorCard memory={MEMORY} history={null} range="24h" />);
    const rows = [...dom.querySelectorAll('tr')];
    const kernel = rows.filter((row) =>
      /major page faults|minor page faults|peak RSS/.test(row.textContent ?? ''),
    );
    expect(kernel).toHaveLength(3);
    for (const row of kernel) {
      expect(row.querySelector('.tagq')).toBeNull();
    }
  });
});

describe('the KPI rail', () => {
  it('measures RSS against the budget and residual against RSS', async () => {
    const dom = await mountPage();
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('% of steady-state budget');
    expect(text).toContain('% of current RSS');
  });

  it('gives peak no budget bar, because peak past a budget is not a breach', async () => {
    const dom = await mountPage();
    const cards = [...dom.querySelectorAll('.kpi')];
    const peak = cards.find((card) =>
      (card.textContent ?? '').includes('Peak RSS'),
    );
    expect(peak).toBeDefined();
    // The fixture's peak is past the steady-state budget on purpose. A rail
    // under it would render that transient as 115 % of budget, which is the
    // reading that invites someone to set `memory-high` — and that has
    // OOM-killed this household's resolver once.
    expect(peak?.querySelector('.rail')).toBeNull();
    expect(peak?.textContent).toContain('not a breach');
  });

  it('gives committed no budget bar either, and says why', async () => {
    const dom = await mountPage();
    const committed = [...dom.querySelectorAll('.kpi')].find((card) =>
      (card.textContent ?? '').includes('Allocator committed'),
    );
    expect(committed?.querySelector('.rail')).toBeNull();
    expect(committed?.querySelector('.tagq')?.textContent).toBe('no contract');
  });

  it('claims no monotone committed — mimalloc v3 was observed decreasing it', async () => {
    const dom = await mountPage();
    const committed = [...dom.querySelectorAll('.kpi')].find((card) =>
      (card.textContent ?? '').includes('Allocator committed'),
    );
    expect(committed?.textContent).not.toContain('only rises');
    expect(committed?.textContent).not.toContain('Equal is the normal state');
    expect(committed?.textContent).toContain('a purge, not a restart');
  });

  it('renders an em dash, never a zero, for a figure nobody reported', () => {
    const dom = mount(
      <KpiRail
        memory={{ ...MEMORY, allocator_committed_bytes: null }}
        items={[]}
        rss={MEMORY.process_rss as number}
        range="24h"
      />,
    );
    const committed = [...dom.querySelectorAll('.kpi')].find((card) =>
      (card.textContent ?? '').includes('Allocator committed'),
    );
    expect(committed?.querySelector('.kpiv')?.textContent).toBe('—');
  });
});

describe('the RSS state', () => {
  // The thresholds are what let the line be red at all: red is a status
  // colour, so it only ever appears beside the labelled rule that names it.
  it('is ink below the watch point', () => {
    expect(rssState(80 * 1024 * 1024, null)).toBe('normal');
  });

  it('is watch above 100 MiB and still under budget', () => {
    expect(rssState(110 * 1024 * 1024, null)).toBe('watch');
  });

  it('is over above the steady-state budget', () => {
    expect(rssState(STEADY_STATE_BUDGET + 1, null)).toBe('over');
  });

  it('does not fall back on the threshold itself', () => {
    // A reading hovering on a threshold would otherwise alternate state every
    // sample, and a line that changes colour every 60 s reads as an event when
    // nothing happened.
    const justUnder = WATCH_THRESHOLD - 1024;
    expect(rssState(justUnder, 'watch')).toBe('watch');
    expect(rssState(justUnder, 'normal')).toBe('normal');
  });

  /**
   * The regression: hysteresis loosens a threshold on the way **down** only,
   * so a first reading has to sit on the plain one. The earlier form tested
   * `previous === 'normal'`, which put `null` on the falling threshold — the
   * card read `watch` from 97 MiB while the chart's rule was captioned at the
   * documented 100 MiB.
   */
  it('holds a first reading to the documented watch point, not 3 MiB under it', () => {
    expect(rssState(WATCH_THRESHOLD - 1024, null)).toBe('normal');
    expect(rssState(WATCH_THRESHOLD, null)).toBe('watch');
  });

  it('holds a first reading to the documented budget too', () => {
    expect(rssState(STEADY_STATE_BUDGET - 1, null)).toBe('watch');
    expect(rssState(STEADY_STATE_BUDGET, null)).toBe('over');
  });

  it('only loosens on the way down, and by one band', () => {
    // In `watch`, it takes the full band to fall out of it…
    expect(rssState(WATCH_THRESHOLD - HYSTERESIS, 'watch')).toBe('watch');
    expect(rssState(WATCH_THRESHOLD - HYSTERESIS - 1, 'watch')).toBe('normal');
    // …and `over` is sticky over the same band without skipping `watch`.
    expect(rssState(STEADY_STATE_BUDGET - HYSTERESIS, 'over')).toBe('over');
    expect(rssState(STEADY_STATE_BUDGET - HYSTERESIS - 1, 'over')).toBe('watch');
  });

  it('carries the state along a series, and gaps do not reset it', () => {
    // The walk the line is drawn from. A `null` row is a sample nobody wrote,
    // not a reading of zero, so it takes no state and loses none.
    expect(
      rssStates([
        80 * 1024 * 1024,
        WATCH_THRESHOLD,
        null,
        WATCH_THRESHOLD - HYSTERESIS,
        WATCH_THRESHOLD - HYSTERESIS - 1,
      ]),
    ).toEqual(['normal', 'watch', null, 'watch', 'normal']);
  });
});

/**
 * The hysteresis, asserted on what the card renders rather than on the pure
 * function or on the source text. The earlier test was a grep, which passed on
 * a variant that computed the state and threw the carry away.
 */
describe('the RSS card across successive readings', () => {
  /** The tone the RSS card renders, given a window of readings whose last
   *  element is the live one. */
  function toneOf(readings: number[]): { figure: string; rail: string } {
    const items = readings
      .slice(0, -1)
      .map((rss_bytes) => perfRow({ rss_bytes }) as PerfItem);
    const dom = mount(
      <KpiRail
        memory={MEMORY}
        items={items}
        rss={readings[readings.length - 1] as number}
        range="24h"
      />,
    );
    const card = dom.querySelector('.kpi');
    return {
      figure: card?.querySelector('.kpiv')?.className ?? '',
      rail: card?.querySelector('.rail-fill')?.className ?? '',
    };
  }

  it('turns watch at the documented 100 MiB, not 3 MiB under it', () => {
    expect(toneOf([80 * 1024 * 1024, WATCH_THRESHOLD - 1024]).figure).toContain(
      'kpiv-ink',
    );
    const crossed = toneOf([80 * 1024 * 1024, WATCH_THRESHOLD]);
    expect(crossed.figure).toContain('kpiv-watch');
    expect(crossed.rail).toContain('rail-watch');
  });

  it('stays watch while the reading falls back inside the 3 MiB band', () => {
    // The strobe the band exists to prevent: two samples either side of the
    // watch point would otherwise alternate the card's colour every 60 s.
    const inside = toneOf([
      WATCH_THRESHOLD,
      WATCH_THRESHOLD - HYSTERESIS,
    ]);
    expect(inside.figure).toContain('kpiv-watch');
    expect(inside.rail).toContain('rail-watch');
  });

  it('drops to ink one byte below the band, and not before', () => {
    expect(
      toneOf([WATCH_THRESHOLD, WATCH_THRESHOLD - HYSTERESIS - 1]).figure,
    ).toContain('kpiv-ink');
  });

  it('holds over across the same band under the budget', () => {
    expect(
      toneOf([80 * 1024 * 1024, STEADY_STATE_BUDGET]).figure,
    ).toContain('kpiv-over');
    expect(
      toneOf([STEADY_STATE_BUDGET, STEADY_STATE_BUDGET - HYSTERESIS]).figure,
    ).toContain('kpiv-over');
    // …and falls to `watch`, never past it to ink.
    expect(
      toneOf([STEADY_STATE_BUDGET, STEADY_STATE_BUDGET - HYSTERESIS - 1])
        .figure,
    ).toContain('kpiv-watch');
  });

  it('gives a first reading with no history the plain threshold', () => {
    expect(toneOf([WATCH_THRESHOLD - 1024]).figure).toContain('kpiv-ink');
    expect(toneOf([WATCH_THRESHOLD]).figure).toContain('kpiv-watch');
  });
});

describe('the page', () => {
  it('asks for one point per sample at 24 h', async () => {
    await mountPage();
    const perf = requests.find((url) => url.startsWith('/api/v1/history/perf'));
    // The endpoint's default is 1000 against 1440 sixty-second samples, which
    // would decimate to a stride of 2 — so the chart would be drawing every
    // other minute while the readout said "every sample".
    expect(perf).toContain('max_points=1440');
  });

  it('holds no timer and subscribes to no event type', async () => {
    await mountPage();
    expect(ROUTE.endpoints).toEqual([]);
    expect(ROUTE.events).toEqual([]);
    // Three one-shot reads on entry and nothing after: the instant, the range,
    // and `/telemetry` for the two Key-metrics rows `/debug/memory` does not
    // carry. The invariant forbids *polling*, not fetching — none of the three
    // starts a timer, and the page declares no endpoint and no event type.
    expect(requests).toHaveLength(3);
    expect(
      requests.filter((url) => url.includes('/telemetry')),
    ).toHaveLength(1);
  });

  it('re-reads the history too when Refresh is pressed', async () => {
    const dom = await mountPage();
    expect(
      requests.filter((url) => url.startsWith('/api/v1/history/perf')),
    ).toHaveLength(1);

    const refresh = dom.querySelector('.hd-refresh') as HTMLButtonElement;
    await act(async () => {
      refresh.click();
    });
    await flush();

    // All three reads move together, because the age beside the button covers
    // the whole page: the trend chart, the four KPI sparklines, the allocator's
    // window min/max and the residual verdict are all drawn from the history.
    expect(
      requests.filter((url) => url.startsWith('/api/v1/history/perf')),
    ).toHaveLength(2);
    expect(requests.filter((url) => url === '/api/v1/debug/memory')).toHaveLength(
      2,
    );
  });

  it('always carries the verdict slot, neutral when history is too short', async () => {
    // The sketch's header pill is present on every visit. Two samples cannot
    // support "stable", so the badge says what is true instead of vanishing.
    const dom = await mountPage();
    const pill = dom.querySelector('.memory-verdict');
    expect(pill?.className).toContain('neutral');
    expect(pill?.textContent).toContain('not enough history');
  });

  it('claims stable only once the window has a shape', async () => {
    const items = Array.from({ length: 8 }, (_, index) =>
      perfRow({ ts: new Date(Date.UTC(2026, 7, 28, 0, index)).toISOString() }),
    );
    const dom = await mountPage(MEMORY, { ...HISTORY, items });
    const pill = dom.querySelector('.memory-verdict');
    expect(pill?.className).toContain('good');
    expect(pill?.textContent).toContain('residual stable');
  });

  /** A row at minute `index` with its own peak and residual. */
  function lifetimeRow(index: number, peak: number, residual: number) {
    const base = perfRow();
    return {
      ...base,
      ts: new Date(Date.UTC(2026, 7, 28, 0, index)).toISOString(),
      peak_rss: peak,
      memory: { ...base.memory, residual_bytes: residual },
    };
  }

  it('does not read a climb across a restart as this process rising', async () => {
    // Six rows of the old process climbing 40 -> 52 MiB, then two rows of a
    // fresh one opening at 18. Judged as one window the thirds say `falling`
    // and a differently-shaped window would say `rising` — either way it is a
    // verdict about three binaries. The new process has too few rows to have a
    // shape, and the badge says so.
    const items = [
      ...[40, 44, 48, 50, 51, 52].map((residual, index) =>
        lifetimeRow(index, 148_000_000, residual * 1_048_576),
      ),
      lifetimeRow(6, 89_000_000, 18 * 1_048_576),
      lifetimeRow(7, 90_000_000, 18 * 1_048_576),
    ];
    const dom = await mountPage(MEMORY, { ...HISTORY, items });
    const pill = dom.querySelector('.memory-verdict');
    expect(pill?.className).toContain('neutral');
    expect(pill?.textContent).toContain('not enough history');
  });

  it('still reads a climb inside one lifetime as rising', async () => {
    // The guard must not swallow the signal it sits beside: past the restart
    // the new process has six rows of its own and a real climb in them shows.
    const items = [
      lifetimeRow(0, 148_000_000, 40 * 1_048_576),
      ...[18, 18, 19, 26, 27, 28].map((residual, index) =>
        lifetimeRow(index + 1, 89_000_000 + index, residual * 1_048_576),
      ),
    ];
    const dom = await mountPage(MEMORY, { ...HISTORY, items });
    const pill = dom.querySelector('.memory-verdict');
    expect(pill?.className).toContain('warn');
    expect(pill?.textContent).toContain('residual rising');
  });

  function overAccountedRow(index: number, peak: number) {
    const base = perfRow();
    return {
      ...base,
      ts: new Date(Date.UTC(2026, 7, 28, 0, index)).toISOString(),
      peak_rss: peak,
      memory: {
        ...base.memory,
        ruleset_bytes: base.rss_bytes + 1,
        residual_bytes: 0,
      },
    };
  }

  it('omits over-accounted rows from the verdict rather than reading their zero', async () => {
    const items = [
      ...[40, 40, 41, 40, 41].map((residual, index) =>
        lifetimeRow(index, 148_000_000, residual * 1_048_576),
      ),
      overAccountedRow(5, 148_000_000),
      overAccountedRow(6, 148_000_000),
      overAccountedRow(7, 148_000_000),
    ];
    const dom = await mountPage(MEMORY, { ...HISTORY, items });
    const pill = dom.querySelector('.memory-verdict');
    expect(pill?.className).toContain('neutral');
    expect(pill?.textContent).toContain('not enough history');
  });

  it('does not read over-accounted zeros in the first third as a climb', async () => {
    const items = [
      overAccountedRow(0, 148_000_000),
      overAccountedRow(1, 148_000_000),
      ...[40, 40, 41, 40, 41, 40].map((residual, index) =>
        lifetimeRow(index + 2, 148_000_000, residual * 1_048_576),
      ),
    ];
    const dom = await mountPage(MEMORY, { ...HISTORY, items });
    const pill = dom.querySelector('.memory-verdict');
    expect(pill?.className).toContain('good');
    expect(pill?.textContent).toContain('residual stable');
  });

  it('does not let over-accounted zeros in the last third hide a climb', async () => {
    const items = [
      ...[40, 44, 48, 50, 51, 52].map((residual, index) =>
        lifetimeRow(index, 148_000_000, residual * 1_048_576),
      ),
      overAccountedRow(6, 148_000_000),
      overAccountedRow(7, 148_000_000),
    ];
    const dom = await mountPage(MEMORY, { ...HISTORY, items });
    const pill = dom.querySelector('.memory-verdict');
    expect(pill?.className).toContain('warn');
    expect(pill?.textContent).toContain('residual rising');
  });

  it('judges six rows by their first two and last two', async () => {
    const middle = [40, 40, 90, 90, 41, 41].map((residual, index) =>
      lifetimeRow(index, 148_000_000, residual * 1_048_576),
    );
    const steady = await mountPage(MEMORY, { ...HISTORY, items: middle });
    expect(steady.querySelector('.memory-verdict')?.textContent).toContain(
      'residual stable',
    );

    const tail = [40, 40, 40, 40, 45, 44].map((residual, index) =>
      lifetimeRow(index, 148_000_000, residual * 1_048_576),
    );
    const climb = await mountPage(MEMORY, { ...HISTORY, items: tail });
    expect(climb.querySelector('.memory-verdict')?.textContent).toContain(
      'residual rising',
    );
  });

  it('cuts the window at a restart the peak alone misses', async () => {
    const old = [40, 44, 48, 50, 51, 52].map((residual, index) => ({
      ...lifetimeRow(index, 148_000_000, residual * 1_048_576),
      minor_page_faults: 5_000_000 + index * 2_000,
    }));
    const fresh = [18, 18].map((residual, index) => ({
      ...lifetimeRow(index + 6, 149_000_000, residual * 1_048_576),
      minor_page_faults: 35_000 + index * 2_000,
    }));
    const dom = await mountPage(MEMORY, {
      ...HISTORY,
      items: [...old, ...fresh],
    });
    const pill = dom.querySelector('.memory-verdict');
    expect(pill?.className).toContain('neutral');
    expect(pill?.textContent).toContain('not enough history');
  });

  it('holds stable through a refresh transient in the last third', async () => {
    const items = [40, 40, 40, 40, 40, 40, 40, 40, 40, 80, 40, 40].map(
      (residual, index) => lifetimeRow(index, 148_000_000, residual * 1_048_576),
    );
    const dom = await mountPage(MEMORY, { ...HISTORY, items });
    const pill = dom.querySelector('.memory-verdict');
    expect(pill?.className).toContain('good');
    expect(pill?.textContent).toContain('residual stable');
  });

  it('states the unit trap the two budgets create', async () => {
    const dom = await mountPage();
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('decimal');
    expect(text).toContain('MiB');
  });

  it('draws both budgets as markers, and neither as a wall', async () => {
    const dom = await mountPage();
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('Budgets, not limits');
    expect(text).toContain('memory-high=unlimited');
    expect(text).toContain(`${CEILING_BUDGET / 1_000_000} MB`);
    expect(text).toContain(`${STEADY_STATE_BUDGET / 1_000_000} MB`);
  });

  it('renders the fault rate as a rate and never as the counter', async () => {
    const dom = await mountPage();
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('118 /s');
    expect(text).toContain('never the counter');
  });

  it('renders an empty range as absence, not as a failure', async () => {
    const dom = await mountPage(MEMORY, { ...HISTORY, items: [] });
    expect(dom.textContent).toContain('No samples in this window');
    expect(dom.querySelector('.error-state')).toBeNull();
  });

  it('renders the unavailable state when RSS is null', async () => {
    const dom = await mountPage({
      ...MEMORY,
      process_rss: null,
      residual_bytes: null,
    });
    expect(dom.textContent).toContain('RSS unavailable on this platform');
    expect(dom.querySelector('.kpi')).toBeNull();
  });

  it('names the window on every card the range chips change', async () => {
    // The chips sit in the chart's header but the window is page state, so
    // four cards move with them: the chart, the KPI sparklines, the fault rate
    // and the allocator's sampled min/max. A card that silently redraws under a
    // control it does not show is a card whose figures cannot be trusted — so
    // each one prints the range it is showing.
    const dom = await mountPage();
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('24 h trend');
    expect(text).toContain('the rate over 24 h');
    expect(text).toContain('sampled series min / max 24 h');
  });

  it('offers the three ranges the retention actually covers', async () => {
    const dom = await mountPage();
    const chips = [...dom.querySelectorAll('.chip')].map(
      (node) => node.textContent,
    );
    // 30 d is the default `history.retention_days`, so it is the widest range
    // guaranteed to hold data.
    expect(chips).toEqual(['24 h', '7 d', '30 d']);
  });
});
