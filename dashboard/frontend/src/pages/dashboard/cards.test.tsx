// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, describe, expect, it } from 'vitest';
import type { HistorySummary, Stats, Telemetry } from '../../api/types';
import { QueriesOverTime } from './queries-over-time';
import { QueryTypes } from './query-types';
import { DnsTiles, HttpTiles } from './tiles';
import { TopList } from './top-list';

/**
 * `Main.dc.html` and `MobileDashboard.dc.html` decide the wording, the column
 * headings and which of the three non-chart states renders. None of that was
 * rendered by a test before the p5-06 review, which is how a shortened phone
 * label, a column heading and an empty-state ordering all reached a browser.
 *
 * The plot itself is not mounted here — uPlot needs a canvas 2D context jsdom
 * does not implement, and its construction count is read off a real browser
 * instead. Every branch below is a branch that draws no plot.
 */

let host: HTMLElement | null = null;

function mount(node: preact.ComponentChild): HTMLElement {
  host = document.createElement('div');
  document.body.append(host);
  act(() => {
    render(node, host as HTMLElement);
  });
  return host;
}

afterEach(() => {
  if (host !== null) {
    render(null, host);
    host.remove();
    host = null;
  }
});

const STATS = {
  window: '24h',
  queries_total: 184233,
  blocked_total: 23411,
  blocked_percent: 12.7,
  cache_hit_percent: 89.7,
  top_blocked_domains: [],
  top_queried_domains: [],
  top_clients: [],
  buckets: [],
  policies: [],
} as Stats;

const TELEMETRY = {
  process: { version: '0.2.20', uptime_seconds: 4 * 3600 + 31 * 60 },
  ruleset: { rules: 752585, duplicates_removed: 87422, compile_duration_seconds: 7.41 },
  counters: {
    http: { pass: 3200, allow: 294, block: 918, response_bytes: 0, refused: 3 },
  },
} as unknown as Telemetry;

function summary(items: HistorySummary['items']): HistorySummary {
  return {
    resolution: 'hour',
    from: '2026-08-26T12:00:00Z',
    to: '2026-08-27T12:00:00Z',
    stride: 1,
    items,
  };
}

const BUCKET = {
  ts: '2026-08-27T04:00:00Z',
  queries: 13169,
  blocked: 1989,
  blocked_percent: 15.1,
  cache_hits: 9000,
  per_type: { A: 8000, AAAA: 3000, HTTPS: 1200, PTR: 700, NS: 269 },
};

describe('tile row 1', () => {
  const tiles = () =>
    mount(<DnsTiles stats={STATS} clientCount={6} />).querySelectorAll('.tile');

  it('is the artboard’s four, in order', () => {
    const labels = Array.from(tiles()).map(
      (tile) => tile.querySelector('.lb-long')?.textContent ?? tile.querySelector('.lb')?.textContent,
    );
    expect(labels).toEqual([
      'Total queries',
      'Queries blocked',
      'Percentage blocked',
      'Cache hit rate',
    ]);
  });

  it('carries the phone artboard’s shortened labels beside the desktop ones', () => {
    // Rendered together and swapped in CSS: a width branch in JS would put a
    // listener on every tile. `MobileDashboard.dc.html` shortens three.
    const short = Array.from(tiles()).map(
      (tile) => tile.querySelector('.lb-short')?.textContent ?? null,
    );
    expect(short).toEqual([null, 'Blocked', 'Blocked', 'Cache hit']);
  });

  it('carries both footer strips', () => {
    const long = Array.from(tiles()).map(
      (tile) => tile.querySelector('.ft-long')?.textContent,
    );
    const short = Array.from(tiles()).map(
      (tile) => tile.querySelector('.ft-short')?.textContent,
    );
    expect(long).toEqual([
      '6 active clients',
      'watch the live feed',
      'of all DNS queries',
      'inspect the cache',
    ]);
    expect(short).toEqual(['6 clients', 'live feed', 'of DNS', 'cache']);
  });

  it('reads every figure from a field', () => {
    const figures = Array.from(tiles()).map(
      (tile) => tile.querySelector('.n')?.textContent,
    );
    expect(figures).toEqual(['184,233', '23,411', '12.7%', '89.7%']);
  });

  it('draws an em dash rather than a zero before the first response', () => {
    const empty = mount(<DnsTiles stats={null} clientCount={null} />);
    expect(empty.querySelector('.n')?.textContent).toBe('—');
    expect(empty.querySelector('.ft-long')?.textContent).toBe('—');
  });
});

describe('tile row 2', () => {
  const mounted = () =>
    mount(<HttpTiles telemetry={TELEMETRY} status="ok" enabledLists={13} />);

  it('sums the HTTP request tile from the three verdicts, refused excluded', () => {
    // API.md: `refused` is counted on the proxy, not on the event stream, so it
    // is not part of `pass + allow + block`.
    const figures = Array.from(mounted().querySelectorAll('.tile .n')).map(
      (node) => node.textContent,
    );
    expect(figures).toEqual(['4,412', '918', '752,585', '4h 31m']);
  });

  it('states the refused count and the health status in the footers', () => {
    const long = Array.from(mounted().querySelectorAll('.tile .ft-long')).map(
      (node) => node.textContent,
    );
    expect(long).toEqual([
      'proxy pipeline',
      '3 refused by egress policy',
      'manage lists',
      'status ok',
    ]);
  });

  it('shortens the compiled-rules footer to the enabled-list count on a phone', () => {
    const short = Array.from(mounted().querySelectorAll('.tile .ft-short')).map(
      (node) => node.textContent,
    );
    expect(short[2]).toBe('13 lists');
  });

  it('says out loud that this row is a different window from the one above', () => {
    // Two visually identical rows over different windows is the trap; the
    // caption is a rendered element rather than a tooltip because a trap nobody
    // hovers is not closed.
    expect(mounted().querySelector('.tile-caption')?.textContent).toContain(
      'since restart',
    );
  });
});

describe('the top-N tables', () => {
  const rows = [{ key: 'a', primary: 'a', count: 10, share: 1 }];

  it('heads the bar column `Frequency` on a domain table', () => {
    const head = mount(<TopList columns={['Domain', 'Hits']} rows={rows} />);
    expect(head.querySelector('.toplist-head .f')?.textContent).toBe('Frequency');
  });

  it('heads it `Share` on Top clients, as `Main.dc.html` draws it', () => {
    const head = mount(
      <TopList
        columns={['Client', 'Queries', 'Blocked']}
        frequencyLabel="Share"
        rows={rows}
      />,
    );
    expect(head.querySelector('.toplist-head .f')?.textContent).toBe('Share');
  });
});

/**
 * The three states §7.4 requires be un-confusable, and the fourth that exists
 * only so two of them are not confused: while the page re-reads `/config` to
 * find out which empty state this is, neither card may name one.
 */
describe('the chart’s non-chart states', () => {
  const chart = (over: {
    summary?: HistorySummary | null;
    loading?: boolean;
    recording?: boolean;
    error?: Error | null;
  }) =>
    mount(
      <QueriesOverTime
        range="24h"
        onRange={() => undefined}
        summary={over.summary ?? null}
        error={over.error ?? null}
        loading={over.loading ?? false}
        recording={over.recording ?? true}
      />,
    );

  it('says recording is off, and hides the range chips with it', () => {
    const off = chart({ recording: false, summary: summary([]) });
    expect(off.textContent).toContain('History is not being recorded');
    // There is no range to pick.
    expect(off.querySelector('.chip')).toBeNull();
  });

  it('says an empty range is empty, not broken, and keeps the chips live', () => {
    const empty = chart({ summary: summary([]) });
    expect(empty.textContent).toContain('No data in this range');
    // Both copies exist at once and CSS hides one per breakpoint: the desktop
    // artboard puts them in the card title bar, the phone one above the plot as
    // full-height rows. `display: none` keeps the hidden copy out of the tab
    // order and the accessibility tree.
    expect(empty.querySelectorAll('.ch-right .chip')).toHaveLength(3);
    expect(empty.querySelectorAll('.chips-mobile .chip')).toHaveLength(3);
  });

  it('names neither empty state while the disambiguation read is in flight', () => {
    // An empty response is exactly when the mount snapshot of
    // `history.enabled` may be stale. Answering before `/config` does flashes
    // "no data in this range" ahead of "history is not being recorded" — the
    // wrong one of the two the task requires be distinguishable.
    const waiting = chart({ summary: summary([]), loading: true });
    expect(waiting.querySelector('.boot')).not.toBeNull();
    expect(waiting.textContent).not.toContain('No data in this range');
    expect(waiting.textContent).not.toContain('History is not being recorded');
  });

  it('keeps the bars up while a range with data is refetched', () => {
    // Only the answer that would *be* an empty state waits; a normal range
    // change must not blank the card for a round trip.
    const busy = chart({ summary: summary([BUCKET]), loading: true });
    expect(busy.querySelector('.boot')).toBeNull();
  });

  it('renders a failed request as an error, not as an empty range', () => {
    const failed = chart({ error: new Error('nope'), summary: summary([]) });
    expect(failed.textContent).toContain('nope');
    expect(failed.textContent).not.toContain('No data in this range');
  });

  it('sums its title totals from the items that draw the bars', () => {
    // Not from `/stats`: that is a rolling 24 h snapshot and states the wrong
    // window at 7 d and 30 d, and a different span even at 24 h.
    const drawn = chart({ summary: summary([BUCKET, BUCKET]) });
    expect(drawn.querySelector('.ch-aside')?.textContent).toBe(
      '26,338 queries · 3,978 blocked · 15.1 %',
    );
  });
});

describe('the query-types donut', () => {
  const donut = (over: {
    summary?: HistorySummary | null;
    loading?: boolean;
    recording?: boolean;
    range?: '24h' | '7d' | '30d';
  }) =>
    mount(
      <QueryTypes
        range={over.range ?? '24h'}
        summary={over.summary ?? null}
        recording={over.recording ?? true}
        loading={over.loading ?? false}
      />,
    );

  const label = (el: HTMLElement) =>
    el.querySelector('.ch-right')?.textContent;

  it('states the range rather than a fixed `last 24 h`', () => {
    expect(label(donut({ summary: summary([BUCKET]) }))).toBe('last 24 h');
    expect(label(donut({ summary: null, range: '30d' }))).toBe('last 30 d');
  });

  it('names the range its own figures cover, not the one being fetched', () => {
    // The chips flip on click; the slices arrive a round trip later. Naming the
    // chip printed `last 24 h` over the 30 d figures for the length of the
    // fetch — the chart's own defect (N2), one card over.
    const thirtyDays = {
      ...summary([BUCKET]),
      resolution: 'day' as const,
      from: '2026-07-28T12:00:00Z',
      to: '2026-08-27T12:00:00Z',
    };
    expect(label(donut({ summary: thirtyDays, range: '24h' }))).toBe(
      'last 30 d',
    );
    // And the ring's own accessible name moves with it, not with the chip.
    expect(
      donut({ summary: thirtyDays, range: '24h' })
        .querySelector('svg')
        ?.getAttribute('aria-label'),
    ).toBe('Query types over the last 30 d');
  });

  it('holds its answer across the disambiguation read too', () => {
    const waiting = donut({ summary: summary([]), loading: true });
    expect(waiting.querySelector('.boot')).not.toBeNull();
    expect(waiting.textContent).not.toContain('No data in this range');
  });

  it('is disabled by the same `history.enabled` as the chart', () => {
    expect(donut({ recording: false }).textContent).toContain(
      'History is not being recorded',
    );
  });

  /**
   * The ring's hover is a **highlight, not a tooltip**, and that is deliberate:
   * the legend beside it already prints every count and share, so an overlay
   * would cover the numbers it repeats. Pointing at either half marks the other.
   */
  describe('marking a slice', () => {
    const enter = (node: Element) =>
      act(() => {
        node.dispatchEvent(new Event('pointerenter'));
      });

    const leave = (node: Element) =>
      act(() => {
        node.dispatchEvent(new Event('pointerleave'));
      });

    it('raises no tooltip', () => {
      const card = donut({ summary: summary([BUCKET]) });
      enter(card.querySelectorAll('.donut-seg')[1] as Element);
      expect(card.querySelector('.chart-tip')).toBeNull();
    });

    it('marks the row the slice names', () => {
      const card = donut({ summary: summary([BUCKET]) });
      enter(card.querySelectorAll('.donut-seg')[1] as Element);
      const marked = Array.from(card.querySelectorAll('.donut-legend tr')).map(
        (row) => row.className,
      );
      expect(marked).toEqual(['', 'on', '', '', '']);
    });

    it('lights the slice the row names', () => {
      // The legend is the larger target, and on a phone the only one a thumb
      // can hit without landing on a 22 px band — so it raises the same state.
      const card = donut({ summary: summary([BUCKET]) });
      enter(card.querySelectorAll('.donut-legend tr')[2] as Element);
      const opacity = Array.from(card.querySelectorAll('.donut-seg')).map((s) =>
        s.getAttribute('opacity'),
      );
      expect(opacity).toEqual(['0.35', '0.35', '1', '0.35', '0.35']);
    });

    it('leaves every slice at full strength with nothing marked', () => {
      const card = donut({ summary: summary([BUCKET]) });
      const seg = card.querySelectorAll('.donut-seg')[1] as Element;
      enter(seg);
      leave(seg);
      const opacity = Array.from(card.querySelectorAll('.donut-seg')).map((s) =>
        s.getAttribute('opacity'),
      );
      expect(opacity).toEqual(['1', '1', '1', '1', '1']);
      expect(card.querySelector('.donut-legend tr.on')).toBeNull();
    });

    it('gives each row its slice’s colour to mark itself with', () => {
      // The accent bar is the slice's own token, so the mark says *which* arc
      // it belongs to rather than merely that something is marked.
      const rows = Array.from(
        donut({ summary: summary([BUCKET]) }).querySelectorAll(
          '.donut-legend tr',
        ),
      );
      expect(
        rows.map((row) => (row as HTMLElement).style.getPropertyValue('--slice')),
      ).toEqual([
        'var(--series-1)',
        'var(--series-2)',
        'var(--series-3)',
        'var(--series-4)',
        'var(--series-5)',
      ]);
    });
  });

  it('prints each slice’s count and its share of the range', () => {
    const legend = donut({ summary: summary([BUCKET]) }).querySelectorAll(
      '.donut-legend tr',
    );
    const read = Array.from(legend).map((tr) =>
      Array.from(tr.children).map((cell) => cell.textContent),
    );
    expect(read).toEqual([
      ['A', '8,000', '60.7%'],
      ['AAAA', '3,000', '22.8%'],
      ['HTTPS', '1,200', '9.1%'],
      ['PTR', '700', '5.3%'],
      ['NS', '269', '2.0%'],
    ]);
  });
});
