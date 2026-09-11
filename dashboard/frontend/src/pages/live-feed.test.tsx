// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Client, QueryEvent } from '../api/types';
import type { Route } from '../router/routes';
import { socket } from '../services';
import LiveFeed from './live-feed';
import { Detail, FeedCache, FeedVerdict } from './live-feed/detail';
import { applyFilters, EMPTY_FILTERS } from './live-feed/filters';
import {
  BoundedRing,
  DESKTOP_CAPACITY,
  FeedBuffer,
  NARROW_CAPACITY,
  ringCapacity,
} from './live-feed/ring';

/**
 * The three properties the ring exists for: it is bounded, it starts empty, and
 * it retains nothing. Plus the two the vocabulary depends on — the cache
 * outcome is its own cell rather than a verdict, and the verdict set is closed.
 */

const ROUTE: Route = {
  path: '/live-feed',
  title: 'Live Feed',
  section: 'overview',
  events: ['query'],
  endpoints: [],
  built: true,
  ownsHeader: true,
  load: null,
};

function event(over: Partial<QueryEvent> = {}): QueryEvent {
  return {
    kind: 'dns',
    ts: '2026-08-28T10:41:03.610Z',
    client: '192.168.10.22',
    client_name: 'desktop',
    domain: 'github.com',
    qtype: 'A',
    verdict: 'pass',
    rule: null,
    list: null,
    duration_ms: 0.1,
    upstream: null,
    cached: false,
    method: null,
    path: null,
    resource_type: null,
    status: null,
    bytes: null,
    ...over,
  };
}

let host: HTMLElement | null = null;

function mount(node: preact.ComponentChild): HTMLElement {
  host = document.createElement('div');
  document.body.append(host);
  act(() => {
    render(node, host as HTMLElement);
  });
  return host;
}

/** The seam: a synchronous scheduler, so a test drives the flush rather than
 *  jsdom's frame clock. */
function immediate(run: () => void): () => void {
  run();
  return () => undefined;
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

describe('the bounded ring', () => {
  it('starts empty', () => {
    const ring = new BoundedRing<number>(4);
    expect(ring.length).toBe(0);
    expect(ring.items()).toEqual([]);
  });

  it('keeps its capacity and drops the oldest', () => {
    const ring = new BoundedRing<number>(500);
    for (let index = 0; index < 600; index += 1) ring.push(index);
    expect(ring.length).toBe(500);
    const items = ring.items();
    expect(items[0]).toBe(100);
    expect(items[items.length - 1]).toBe(599);
  });

  it('never grows with the number of events pushed', () => {
    // The whole point: memory is fixed whatever the query rate or the uptime.
    const ring = new BoundedRing<number>(8);
    for (let index = 0; index < 100_000; index += 1) ring.push(index);
    expect(ring.length).toBe(8);
    expect(ring.items()).toHaveLength(8);
  });

  it('empties on clear', () => {
    const ring = new BoundedRing<number>(4);
    ring.push(1);
    ring.clear();
    expect(ring.length).toBe(0);
    expect(ring.items()).toEqual([]);
  });
});

describe('the capacity choice', () => {
  it('is 500 on desktop and 200 on a narrow viewport', () => {
    expect(ringCapacity(() => false)).toBe(DESKTOP_CAPACITY);
    expect(ringCapacity(() => true)).toBe(NARROW_CAPACITY);
    expect(DESKTOP_CAPACITY).toBe(500);
    expect(NARROW_CAPACITY).toBe(200);
  });

  it('asks the phone breakpoint, so the ring and the layout switch together', () => {
    const asked: string[] = [];
    ringCapacity((query) => {
      asked.push(query);
      return false;
    });
    expect(asked).toEqual(['(max-width: 767px)']);
  });
});

describe('the frame-coalesced buffer', () => {
  it('renders once per frame however many events arrive', () => {
    const flushes: number[] = [];
    const frames: Array<() => void> = [];
    const buffer = new FeedBuffer<number>(
      10,
      (items) => flushes.push(items.length),
      (run) => {
        frames.push(run);
        return () => undefined;
      },
    );
    for (let index = 0; index < 50; index += 1) buffer.push(index);
    // Fifty events, one scheduled frame — and nothing rendered until it runs.
    expect(frames).toHaveLength(1);
    expect(flushes).toEqual([]);
    frames[0]?.();
    expect(flushes).toEqual([10]);
  });

  it('absorbs into the ring before the frame runs', () => {
    const buffer = new FeedBuffer<number>(4, () => undefined, () => () => undefined);
    for (let index = 0; index < 9; index += 1) buffer.push(index);
    expect(buffer.length).toBe(4);
  });

  it('renders the empty result at once on clear', () => {
    const flushes: number[] = [];
    const buffer = new FeedBuffer<number>(4, (items) => flushes.push(items.length), immediate);
    buffer.push(1);
    buffer.clear();
    expect(flushes).toEqual([1, 0]);
    expect(buffer.length).toBe(0);
  });

  it('releases the pending frame on dispose', () => {
    let cancelled = false;
    const buffer = new FeedBuffer<number>(4, () => undefined, () => {
      return () => {
        cancelled = true;
      };
    });
    buffer.push(1);
    buffer.dispose();
    expect(cancelled).toBe(true);
  });
});

describe('the filters', () => {
  const rows = [
    event({ verdict: 'pass', domain: 'github.com', client_name: 'desktop' }),
    event({ verdict: 'block', domain: 'ads.example.com', client_name: 'tv' }),
    event({
      kind: 'http',
      verdict: 'pass',
      domain: 'cdn.example.net',
      client_name: null,
      client: '192.168.10.50',
    }),
  ];

  it('passes everything through when nothing is set', () => {
    expect(applyFilters(rows, EMPTY_FILTERS)).toHaveLength(3);
  });

  it('matches a verdict exactly', () => {
    const found = applyFilters(rows, { ...EMPTY_FILTERS, verdict: 'block' });
    expect(found.map((row) => row.domain)).toEqual(['ads.example.com']);
  });

  it('matches a pipeline exactly', () => {
    expect(applyFilters(rows, { ...EMPTY_FILTERS, kind: 'http' })).toHaveLength(1);
  });

  it('matches a client by name or by address', () => {
    expect(applyFilters(rows, { ...EMPTY_FILTERS, client: 'tv' })).toHaveLength(1);
    expect(
      applyFilters(rows, { ...EMPTY_FILTERS, client: '192.168.10.50' }),
    ).toHaveLength(1);
  });

  it('matches a domain by substring, case-insensitively', () => {
    expect(applyFilters(rows, { ...EMPTY_FILTERS, domain: 'EXAMPLE' })).toHaveLength(
      2,
    );
  });

  it('combines the four as an intersection', () => {
    expect(
      applyFilters(rows, {
        verdict: 'pass',
        kind: 'dns',
        client: 'desk',
        domain: 'github',
      }),
    ).toHaveLength(1);
  });
});

describe('the row vocabulary', () => {
  it('gives each of the three verdicts a pill', () => {
    for (const verdict of ['pass', 'allow', 'block']) {
      const dom = mount(<FeedVerdict verdict={verdict} />);
      expect(dom.querySelector('.pill')?.textContent).toBe(verdict);
      act(() => {
        render(null, host as HTMLElement);
      });
      host?.remove();
      host = null;
    }
  });

  it('renders an unexpected verdict literally and gives it no tone', () => {
    // The vocabulary is closed: `ports.rs::verdict_str` answers three values,
    // and REFUSED is a proxy counter that never reaches this stream.
    const dom = mount(<FeedVerdict verdict="refused" />);
    expect(dom.querySelector('.pill')).toBeNull();
    expect(dom.textContent).toBe('refused');
  });

  it('draws the cache outcome in its own cell, never as a verdict', () => {
    const hit = mount(<FeedCache row={event({ cached: true })} />);
    expect(hit.textContent).toBe('HIT');
    expect(hit.querySelector('.pill')).toBeNull();

    const miss = mount(<FeedCache row={event({ cached: false })} />);
    expect(miss.textContent).toBe('MISS');
  });

  /**
   * A lookup that never happened is not a miss. HTTP has no cache to consult,
   * and a blocked DNS query never reaches one (ADR-0001) — which is the same
   * reason `cache_hits + cache_misses` counts `pass + allow` and never `block`.
   */
  it('shows no cache outcome where the cache was never asked', () => {
    const http = mount(<FeedCache row={event({ kind: 'http', cached: false })} />);
    expect(http.textContent).toBe('—');

    const blocked = mount(
      <FeedCache row={event({ verdict: 'block', cached: false })} />,
    );
    expect(blocked.textContent).toBe('—');
  });

  it('shows `endpoint N` only when the key is present', () => {
    const forwarded = mount(<Detail row={event({ endpoint: 1 })} />);
    expect(forwarded.textContent).toContain('endpoint 1');
    act(() => {
      render(null, host as HTMLElement);
    });
    host?.remove();
    host = null;

    const blocked = mount(<Detail row={event({ verdict: 'block' })} />);
    expect(blocked.textContent).not.toContain('endpoint');
  });

  it('composes the HTTP detail from the four documented fields', () => {
    const dom = mount(
      <Detail
        row={event({
          kind: 'http',
          method: 'GET',
          resource_type: 'image',
          status: 200,
          bytes: 0,
          qtype: null,
        })}
      />,
    );
    expect(dom.textContent).toBe('GET · image · 200 · 0 B');
  });

  /**
   * An intercepted HTTPS request carries the same shape as an HTTP one — the
   * wire builds both through `ports.rs::as_http` — so it must read the same.
   * Branching on `kind === 'http'` sent it through the DNS branch instead,
   * where it drew an empty record type and lost method, status and bytes.
   */
  it('draws an intercepted HTTPS request like an HTTP one', () => {
    const dom = mount(
      <Detail
        row={event({
          kind: 'https',
          method: 'GET',
          resource_type: 'script',
          status: 200,
          bytes: 1024,
          qtype: null,
        })}
      />,
    );
    expect(dom.textContent).toBe('GET · script · 200 · 1 KiB');
  });

  /**
   * The SNI leg decides before a request exists. `session_event` fills method
   * and path with empty strings and status with 0 as placeholders, so only the
   * relayed byte count is a real reading and only it is drawn.
   */
  it('draws only the relayed bytes on an SNI row, never placeholder fields', () => {
    const dom = mount(
      <Detail
        row={event({
          kind: 'https-sni',
          method: '',
          path: '',
          resource_type: 'unknown',
          status: 0,
          bytes: 2048,
          qtype: null,
        })}
      />,
    );
    expect(dom.textContent).toBe('2 KiB');
  });

  /**
   * `cached: false` on a Phase 3 row means the cache was never asked, not that
   * it missed. Both kinds must read as "no outcome", exactly like HTTP.
   */
  it('shows no cache outcome on either Phase 3 kind', () => {
    for (const kind of ['https-sni', 'https']) {
      const dom = mount(<FeedCache row={event({ kind, cached: false })} />);
      expect(dom.textContent).toBe('—');
      act(() => {
        render(null, host as HTMLElement);
      });
      host?.remove();
      host = null;
    }
  });
});

function names(count: number): string[] {
  return Array.from({ length: count }, (_, index) => `q${index + 1}.example.com`);
}

/**
 * The page with its `query` listener in reach.
 *
 * `SocketManager` has no emit seam, so the listener is captured off `socket.on`
 * — which is the same registration the shell's route transition drives — and
 * the animation frame is run inline. Both are seams the production path keeps:
 * nothing about the page changes to be testable.
 */
function live({
  narrow = false,
  answer,
}: {
  narrow?: boolean;
  /** How `/api/v1/interception` answers, for the rejection view. Absent means
   *  the rows view, which reads no endpoint at all — so any call is one too
   *  many and the stub says so. */
  answer?: (url: string, init?: RequestInit) => Promise<Response>;
} = {}) {
  let deliver: ((data: Record<string, unknown>) => void) | null = null;
  // Sampled once at mount for the ring's bound, and **subscribed to** for the
  // layout: the page builds one tree, so which one has to follow the
  // breakpoint. Every call shares this state, which is what a real
  // `MediaQueryList` for one query does too.
  let matches = narrow;
  const breakpoint = new Set<() => void>();
  vi.stubGlobal('matchMedia', (query: string) => ({
    get matches() {
      return matches;
    },
    media: query,
    addEventListener: (_type: string, handle: () => void) =>
      breakpoint.add(handle),
    removeEventListener: (_type: string, handle: () => void) =>
      breakpoint.delete(handle),
  }));
  const on = vi.spyOn(socket, 'on').mockImplementation((type, listener) => {
    if (type === 'query') deliver = listener as typeof deliver;
    return () => undefined;
  });
  // The rows view reads no endpoint at all, so any call at all is one too many.
  const requests: string[] = [];
  const fetched = vi.fn((url: string, init?: RequestInit) => {
    requests.push(`${init?.method ?? 'GET'} ${url}`);
    if (answer === undefined) return Promise.reject(new Error('the feed fetches'));
    return answer(url, init);
  });
  vi.stubGlobal('fetch', fetched);
  vi.stubGlobal('requestAnimationFrame', (run: () => void) => {
    run();
    return 1;
  });
  vi.stubGlobal('cancelAnimationFrame', () => undefined);

  const dom = mount(<LiveFeed route={ROUTE} />);
  const press = (node: Element | undefined) => {
    act(() => {
      node?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
  };

  return {
    dom,
    deliver(...rows: (string | Partial<QueryEvent>)[]) {
      act(() => {
        for (const row of rows) {
          const over = typeof row === 'string' ? { domain: row } : row;
          deliver?.(event(over) as unknown as Record<string, unknown>);
        }
      });
    },
    domains: () =>
      [...dom.querySelectorAll('.feed-table tbody .feed-domain')].map(
        (cell) => cell.textContent,
      ),
    cardDomains: () =>
      [...dom.querySelectorAll('.feed-cards .ev-domain')].map((cell) => cell.textContent),
    pager: () => dom.querySelector('.feed-pager')?.textContent ?? '',
    button(label: string): HTMLButtonElement {
      const found = [...dom.querySelectorAll('button')].find(
        (node) => (node.textContent ?? '').trim() === label,
      );
      expect(found, label).toBeDefined();
      return found as HTMLButtonElement;
    },
    click(label: string) {
      press(this.button(label));
    },
    chip(label: string) {
      press(
        [...dom.querySelectorAll('.chip')].find(
          (node) => (node.textContent ?? '').trim() === label,
        ),
      );
    },
    type(selector: string, text: string) {
      const input = dom.querySelector(selector) as HTMLInputElement;
      act(() => {
        input.value = text;
        input.dispatchEvent(new Event('input', { bubbles: true }));
      });
    },
    /** How many times the page registered a socket listener, and how many
     *  requests it made. Both must be flat across a resize. */
    subscriptions: () => on.mock.calls.length,
    requests: () => fetched.mock.calls.length,
    /** Every request the page made, in order — what the exclude action's
     *  "read, append, write the whole document back" is read off. */
    log: () => [...requests],
    bodies: () =>
      fetched.mock.calls
        .filter((call) => (call[1] as RequestInit | undefined)?.body !== undefined)
        .map((call) => JSON.parse(String((call[1] as RequestInit).body)) as unknown),
    /** The window dragged across 768 px, without leaving the page. */
    resize(next: boolean) {
      act(() => {
        matches = next;
        for (const handle of breakpoint) handle();
      });
    },
    release: () => on.mockRestore(),
  };
}

describe('the page', () => {
  function mountPage(): HTMLElement {
    return mount(<LiveFeed route={ROUTE} />);
  }

  it('starts empty and says why', () => {
    const dom = mountPage();
    expect(dom.textContent).toContain('Starts empty');
    expect(dom.querySelector('.feed-table')).toBeNull();
  });

  it('never calls itself a query log', () => {
    const dom = mountPage();
    expect(dom.textContent).not.toContain('Query Log');
  });

  it('offers exactly the closed verdict vocabulary as chips', () => {
    const dom = mountPage();
    const chips = [...dom.querySelectorAll('.feed-chipset .chip')].map(
      (node) => node.textContent,
    );
    expect(chips).toEqual([
      // The view row comes first — the rejection view is a reading of the same
      // ring, so it sits above the filters that narrow it rather than among
      // them (ADR-0008 §The operator's path).
      'all traffic',
      'Certificate rejected by client',
      'all',
      'pass',
      'allow',
      'block',
      'all',
      'dns',
      'http',
      'https-sni',
      'https',
    ]);
    expect(chips).not.toContain('refused');
  });

  it('holds the query subscription in the route table, not in the page', () => {
    // The shell's route transition acquires and releases it; a page that
    // subscribed itself would keep the engine publishing after a navigation.
    expect(ROUTE.events).toEqual(['query']);
    expect(ROUTE.endpoints).toEqual([]);
  });

  it('shows the ring meter as held over capacity', () => {
    const dom = mountPage();
    expect(dom.textContent).toContain('0 / 500 rows held');
  });

  it('renders the newest row first', () => {
    // A tail is read from the top. The ring hands its rows back oldest-first,
    // which is the order a buffer has and not the order a feed is read in.
    const feed = live();
    feed.deliver('first.example.com', 'second.example.com', 'newest.example.com');

    expect(feed.domains()).toEqual([
      'newest.example.com',
      'second.example.com',
      'first.example.com',
    ]);
    expect(feed.dom.textContent).toContain('3 / 500 rows held');
    feed.release();
  });

  it('colours a duration that waited, and reddens a much slower one', () => {
    const feed = live();
    feed.deliver(
      { domain: 'quick.example.com', duration_ms: 50 },
      { domain: 'slow.example.com', duration_ms: 50.001 },
      { domain: 'edge.example.com', duration_ms: 100 },
      { domain: 'stalled.example.com', duration_ms: 100.001 },
    );

    // Newest first, and neither boundary is over itself: 100 is amber, not
    // red, exactly as 50 is plain, not amber.
    expect(
      [...feed.dom.querySelectorAll('.feed-table tbody tr')].map(
        (row) => row.lastElementChild?.className,
      ),
    ).toEqual([
      'num mono feed-ms-bad',
      'num mono feed-ms-warn',
      'num mono feed-ms-warn',
      'num mono',
    ]);
    feed.release();
  });

  it('colours the phone card as well — it is the same feed', () => {
    const feed = live({ narrow: true });
    feed.deliver(
      { domain: 'quick.example.com', duration_ms: 50 },
      { domain: 'slow.example.com', duration_ms: 50.001 },
      { domain: 'edge.example.com', duration_ms: 100 },
      { domain: 'stalled.example.com', duration_ms: 100.001 },
    );

    expect(
      [...feed.dom.querySelectorAll('.feed-cards .ev')].map(
        (card) =>
          [...card.querySelectorAll('.ev-meta span')].find((span) =>
            (span.textContent ?? '').endsWith(' ms'),
          )?.className,
      ),
    ).toEqual([
      'mono feed-ms-bad',
      'mono feed-ms-warn',
      'mono feed-ms-warn',
      'mono',
    ]);
    feed.release();
  });

  it('builds one tree, not both — the table at a wide viewport', () => {
    // `display: none` is not "out of the tree": building the ten-column table
    // *and* the card list for every visible row doubled exactly the per-flush
    // cost the frame coalescing exists to bound.
    const feed = live();
    feed.deliver('first.example.com', 'second.example.com');
    expect(feed.domains()).toHaveLength(2);
    expect(feed.dom.querySelector('.feed-cards')).toBeNull();
    feed.release();
  });

  it('builds the cards and no table on a narrow viewport', () => {
    const feed = live({ narrow: true });
    feed.deliver('first.example.com', 'second.example.com');
    expect(feed.cardDomains()).toEqual([
      'second.example.com',
      'first.example.com',
    ]);
    expect(feed.dom.querySelector('.feed-table')).toBeNull();
    // The same one sample sizes the ring, so the two cannot disagree.
    expect(feed.dom.textContent).toContain('2 / 200 rows held');
    feed.release();
  });

  it('switches the tree it builds when the viewport crosses the breakpoint', () => {
    // One tree means the CSS hides the only one there is the moment the window
    // crosses 768 px: the table under `display: none` with no cards beside it
    // left the pager counting rows over an empty body.
    const feed = live();
    feed.deliver('first.example.com');
    expect(feed.dom.querySelector('.feed-table')).not.toBeNull();

    feed.resize(true);
    expect(feed.dom.querySelector('.feed-table')).toBeNull();
    expect(feed.cardDomains()).toEqual(['first.example.com']);

    feed.resize(false);
    expect(feed.dom.querySelector('.feed-cards')).toBeNull();
    expect(feed.domains()).toEqual(['first.example.com']);
    feed.release();
  });

  it('keeps the ring at the bound it opened with across that resize', () => {
    // The bound is memory, not layout: resizing a window is not a reason to
    // reallocate one, and the page states the figure it opened with — in both
    // directions and at both ends of the trip.
    const feed = live();
    feed.deliver('first.example.com');
    feed.resize(true);
    expect(feed.dom.textContent).toContain('1 / 500 rows held');
    feed.resize(false);
    expect(feed.dom.textContent).toContain('1 / 500 rows held');
    feed.release();
  });

  it('makes no request and no subscription when the viewport changes', () => {
    // The listener is registered against the buffer, and the buffer is keyed on
    // the capacity — which the resize does not move. A rebuilt buffer would
    // re-register the socket listener and drop the ring on the floor.
    const feed = live();
    feed.deliver('first.example.com');
    const subscriptions = feed.subscriptions();
    feed.resize(true);
    feed.resize(false);
    expect(feed.subscriptions()).toBe(subscriptions);
    expect(feed.requests()).toBe(0);
    // The rows are still the ones the ring absorbed before the resize.
    expect(feed.domains()).toEqual(['first.example.com']);
    feed.release();
  });

  it('keys a row by its sequence, so a key cannot change meaning', () => {
    // The key was `${ts}-${pageIndex}`. The list is reversed and paged, so a
    // new event shifted every index and the same DOM node was reused for a
    // different event.
    const feed = live();
    feed.deliver('first.example.com');
    const firstNode = feed.dom.querySelector('.feed-table tbody tr');
    feed.deliver('second.example.com');
    const rows = [...feed.dom.querySelectorAll('.feed-table tbody tr')];
    // `first` moved from row 0 to row 1 and kept its node; the new event got a
    // new one.
    expect(rows[1]).toBe(firstNode);
    expect(rows[0]).not.toBe(firstNode);
    feed.release();
  });
});

describe('paging the held rows', () => {
  it('draws one page of 50 by default, newest first', () => {
    const feed = live();
    feed.deliver(...names(120));
    expect(feed.domains()).toHaveLength(50);
    // 120 arrived, so the newest is 120 and the 50th row back is 71.
    expect(feed.domains()[0]).toBe('q120.example.com');
    expect(feed.domains()[49]).toBe('q71.example.com');
    expect(feed.pager()).toContain('rows 1–50 of 120');
    expect(feed.pager()).toContain('page 1 of 3');
    feed.release();
  });

  it('offers exactly 50, 100 and 200 as page sizes', () => {
    const feed = live();
    feed.deliver(...names(10));
    expect(
      [...feed.dom.querySelectorAll('.feed-page-sizes .chip')].map((c) => c.textContent),
    ).toEqual(['50', '100', '200']);
    feed.release();
  });

  it('redraws at the chosen size and returns to the newest page', () => {
    const feed = live();
    feed.deliver(...names(120));
    feed.click('Older');
    expect(feed.pager()).toContain('page 2 of 3');
    feed.chip('100');
    expect(feed.domains()).toHaveLength(100);
    expect(feed.pager()).toContain('rows 1–100 of 120');
    expect(feed.pager()).toContain('page 1 of 2');
    feed.chip('200');
    expect(feed.domains()).toHaveLength(120);
    expect(feed.pager()).toContain('page 1 of 1');
    feed.release();
  });

  it('walks older and newer, and stops at both ends', () => {
    const feed = live();
    feed.deliver(...names(120));
    expect(feed.button('Newer').disabled).toBe(true);
    feed.click('Older');
    expect(feed.domains()[0]).toBe('q70.example.com');
    expect(feed.button('Newer').disabled).toBe(false);
    feed.click('Older');
    expect(feed.pager()).toContain('rows 101–120 of 120');
    expect(feed.button('Older').disabled).toBe(true);
    feed.click('Newer');
    feed.click('Newer');
    expect(feed.pager()).toContain('page 1 of 3');
    feed.release();
  });

  it('draws no pager while one page holds everything', () => {
    const feed = live();
    feed.deliver(...names(10));
    expect(feed.pager()).toContain('page 1 of 1');
    expect(feed.button('Older').disabled).toBe(true);
    expect(feed.button('Newer').disabled).toBe(true);
    feed.release();
  });

  it('returns to the newest page when a filter narrows the set', () => {
    const feed = live();
    feed.deliver(...names(120));
    feed.click('Older');
    expect(feed.pager()).toContain('page 2 of 3');
    feed.chip('block');
    // Nothing matches, so the empty state replaces the table — and the page
    // index went back to the newest rather than staying past the end.
    expect(feed.dom.textContent).toContain('No held row matches these filters');
    feed.chip('all');
    expect(feed.pager()).toContain('page 1 of 3');
    feed.release();
  });

  it('says the count of matching rows against the count held', () => {
    const feed = live();
    feed.deliver(...names(60));
    feed.deliver('ads.example.com');
    feed.type('#feed-domain', 'ads');
    expect(feed.pager()).toContain('rows 1–1 of 1 matching, 61 held');
    feed.release();
  });
});

/**
 * The rejection view — the same ring, read as one decision per client and host,
 * with the one write ADR-0008 allows: exclude the exact host observed, after a
 * confirmation that names it.
 */
describe('the rejection view', () => {
  function reply(status: number, body: unknown): Response {
    return {
      ok: status >= 200 && status < 300,
      status,
      headers: { get: () => null },
      json: async () => body,
    } as unknown as Response;
  }

  function answered(
    document: { clients: string[]; exclude_domains: string[] },
    put?: { status: number; body: unknown },
    known: Client[] = [],
  ) {
    return (url: string, init?: RequestInit): Promise<Response> => {
      if (url === '/api/v1/clients') {
        return Promise.resolve(reply(200, { items: known }));
      }
      if (url !== '/api/v1/interception') {
        return Promise.reject(new Error(`unexpected ${url}`));
      }
      if (init?.method === 'PUT') {
        const sent = JSON.parse(String(init.body)) as typeof document;
        return Promise.resolve(
          put === undefined ? reply(200, sent) : reply(put.status, put.body),
        );
      }
      return Promise.resolve(reply(200, document));
    };
  }

  async function settle(): Promise<void> {
    await act(async () => {
      for (let turn = 0; turn < 12; turn += 1) await Promise.resolve();
    });
  }

  function rejection(over: Partial<QueryEvent> = {}): Partial<QueryEvent> {
    return { kind: 'https', status: 525, verdict: 'pass', ...over };
  }

  async function open(
    options: Parameters<typeof live>[0] = {},
  ): Promise<ReturnType<typeof live>> {
    const feed = live(options);
    feed.chip('Certificate rejected by client');
    await settle();
    return feed;
  }

  function groupRows(feed: ReturnType<typeof live>): string[] {
    return [...feed.dom.querySelectorAll('.feed-card tbody tr')].map(
      (row) => row.textContent ?? '',
    );
  }

  it('shows one row per client and host, and nothing that is not a 525', async () => {
    const feed = await open({
      answer: answered({ clients: [], exclude_domains: [] }),
    });
    feed.deliver(
      rejection({ domain: 'x.example', ts: '2026-09-10T10:00:00.000Z' }),
      rejection({ domain: 'x.example', ts: '2026-09-10T10:00:01.000Z' }),
      rejection({ domain: 'x.example', ts: '2026-09-10T10:00:02.000Z' }),
      rejection({
        domain: 'x.example',
        client: '192.168.10.99',
        client_name: null,
        ts: '2026-09-10T10:00:03.000Z',
      }),
      { kind: 'https', status: 200, domain: 'fine.example' },
      { kind: 'https-sni', verdict: 'block', domain: 'blocked.example' },
    );

    const rows = groupRows(feed);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toContain('192.168.10.99');
    expect(rows[1]).toContain('desktop');
    expect(feed.dom.textContent).not.toContain('fine.example');
    expect(feed.dom.textContent).not.toContain('blocked.example');
    feed.release();
  });

  it("states the engine's completed-handshake count per client, never CA state", async () => {
    const feed = await open({
      answer: answered({ clients: [], exclude_domains: [] }, undefined, [
        {
          ip: '192.168.10.22',
          name: null,
          first_seen: '2026-09-10T09:00:00Z',
          last_seen: '2026-09-10T10:00:00Z',
          queries_24h: 0,
          blocked_24h: 0,
          policy: 'default',
          intercepted: {
            completed: 14,
            rejected: 2,
            last_completed: '2026-09-10T09:59:00Z',
            last_rejected: null,
          },
        },
      ]),
    });
    await settle();
    feed.deliver(
      rejection({ domain: 'x.example', ts: '2026-09-10T10:00:01Z' }),
      rejection({
        domain: 'x.example',
        client: '192.168.10.99',
        client_name: null,
        ts: '2026-09-10T10:00:02Z',
      }),
      rejection({ domain: 'y.example', ts: '2026-09-10T10:00:03Z' }),
    );
    const text = feed.dom.textContent ?? '';
    expect(text).toContain('Completed intercepted handshakes on record');
    const lines = [...feed.dom.querySelectorAll('.rejection-clients p')].map(
      (line) => line.textContent ?? '',
    );
    expect(lines).toHaveLength(2);
    expect(lines[0]).toContain('desktop');
    expect(lines[0]).toContain('14, last');
    expect(lines[0]).not.toContain('none');
    expect(lines[1]).toContain('192.168.10.99');
    expect(lines[1]).toContain('none');
    expect(lines[1]).toContain('/api/v1/certificates');
    expect(text.toLowerCase()).not.toContain('ca is installed');
    expect(text.toLowerCase()).not.toContain('trusts');
    feed.release();
  });

  it('never says pinned', async () => {
    // ADR-0008 fixes the wording: the label states what was observed, and no
    // screen claims to know why the client refused.
    const feed = await open({
      answer: answered({ clients: [], exclude_domains: [] }),
    });
    feed.deliver(rejection({ domain: 'x.example' }));
    expect((feed.dom.textContent ?? '').toLowerCase()).not.toContain('pinned');
    feed.release();
  });

  it('excludes exactly the observed host, as one whole-document PUT', async () => {
    const feed = await open({
      answer: answered({
        clients: ['192.168.88.0/24'],
        exclude_domains: ['bank.ro'],
      }),
    });
    feed.deliver(rejection({ domain: 'api.bank.example' }));

    feed.click('Exclude');
    expect(feed.dom.textContent).toContain(
      'Exclude api.bank.example from interception?',
    );
    feed.click('Exclude this host');
    await settle();

    // Read, append, write the whole document back — and the read is fresh, so
    // another tab's entries are not clobbered by this browser's stale copy.
    expect(feed.log()).toEqual([
      'GET /api/v1/clients',
      'GET /api/v1/interception',
      'GET /api/v1/interception',
      'PUT /api/v1/interception',
    ]);
    expect(feed.bodies()).toEqual([
      {
        clients: ['192.168.88.0/24'],
        exclude_domains: ['bank.ro', 'api.bank.example'],
      },
    ]);
    // The stored document comes back, so the row stops offering the button.
    expect(groupRows(feed)[0]).toContain('excluded');
    feed.release();
  });

  it('sends nothing when the confirmation is cancelled', async () => {
    const feed = await open({
      answer: answered({ clients: [], exclude_domains: [] }),
    });
    feed.deliver(rejection({ domain: 'api.bank.example' }));
    feed.click('Exclude');
    feed.click('Cancel');
    await settle();

    expect(feed.log()).toEqual(['GET /api/v1/clients', 'GET /api/v1/interception']);
    expect(feed.dom.textContent).not.toContain('Exclude this host');
    feed.release();
  });

  it('shows a host already covered as excluded, with no button', async () => {
    const feed = await open({
      answer: answered({ clients: [], exclude_domains: ['bank.example'] }),
    });
    // Under a parent entry, which covers itself and every subdomain — the
    // `ExclusionSet` walk, so the view and the matcher agree.
    feed.deliver(rejection({ domain: 'api.bank.example' }));

    expect(groupRows(feed)[0]).toContain('excluded');
    expect(
      [...feed.dom.querySelectorAll('.feed-card button')].map((node) =>
        (node.textContent ?? '').trim(),
      ),
    ).not.toContain('Exclude');
    feed.release();
  });

  it('judges a duplicate rejection against the fresh document it is about', async () => {
    // Another tab excluded the host after this view opened. The read before
    // the write returns a document that already covers it, the `PUT` earns a
    // `duplicate`, and the fresh copy — not the mounted one — is what the row
    // is judged against: it reads "excluded", since a button could only fail
    // the same way again.
    let reads = 0;
    const feed = await open({
      answer: (url, init) => {
        if (url !== '/api/v1/interception') {
          return Promise.reject(new Error(`unexpected ${url}`));
        }
        if (init?.method === 'PUT') {
          return Promise.resolve(
            reply(422, {
              error: {
                code: 'validation_failed',
                message: 'exclude_domains[1] duplicates entry 0',
                details: {
                  reason: 'duplicate',
                  list: 'exclude_domains',
                  index: 1,
                  entry: 'api.bank.example',
                  duplicate_of: 0,
                },
              },
            }),
          );
        }
        reads += 1;
        return Promise.resolve(
          reply(200, {
            clients: [],
            exclude_domains: reads === 1 ? [] : ['api.bank.example'],
          }),
        );
      },
    });
    feed.deliver(rejection({ domain: 'api.bank.example' }));
    feed.click('Exclude');
    feed.click('Exclude this host');
    await settle();

    expect(feed.log().filter((entry) => entry.startsWith('PUT'))).toHaveLength(1);
    expect(groupRows(feed)[0]).toContain('excluded');
    expect(groupRows(feed)[0]).not.toContain('Already excluded');
    expect(feed.dom.querySelectorAll('.feed-card tbody button')).toHaveLength(0);
    feed.release();
  });

  it('offers one exclusion at a time, every other row waiting on the write', async () => {
    // Two read-append-PUT sequences in flight would carry the same base, and
    // the later `PUT` would drop the earlier host. So while one is in flight
    // no other row is offered — and once it lands, the rest are again.
    let land = (): void => {};
    const feed = await open({
      answer: (url, init) => {
        if (url === '/api/v1/clients') {
          return Promise.resolve(reply(200, { items: [] }));
        }
        if (init?.method === 'PUT') {
          const sent = JSON.parse(String(init.body)) as {
            clients: string[];
            exclude_domains: string[];
          };
          return new Promise<Response>((resolve) => {
            land = () => resolve(reply(200, sent));
          });
        }
        return Promise.resolve(
          reply(200, { clients: [], exclude_domains: [] }),
        );
      },
    });
    feed.deliver(
      rejection({ domain: 'a.example', ts: '2026-09-10T10:00:01.000Z' }),
      rejection({ domain: 'b.example', ts: '2026-09-10T10:00:00.000Z' }),
    );
    const buttons = () => [
      ...feed.dom.querySelectorAll<HTMLButtonElement>('.feed-card tbody button'),
    ];
    expect(buttons().map((button) => button.disabled)).toEqual([false, false]);

    feed.click('Exclude');
    feed.click('Exclude this host');
    await settle();
    expect(buttons().map((button) => button.disabled)).toEqual([true, true]);

    await act(async () => {
      land();
      await Promise.resolve();
    });
    await settle();
    expect(groupRows(feed)[0]).toContain('excluded');
    expect(buttons().map((button) => button.disabled)).toEqual([false]);
    feed.release();
  });

  it('keeps the groups when the document cannot be read, and retries on request', async () => {
    // The groups are the ring's and need no document; only the action does. A
    // store hiccup must not blank the surface the operator came for.
    let reads = 0;
    const feed = await open({
      answer: (url, init) => {
        if (url === '/api/v1/clients') {
          return Promise.resolve(reply(200, { items: [] }));
        }
        if (init?.method === 'PUT') {
          return Promise.reject(new Error('no PUT expected'));
        }
        reads += 1;
        return Promise.resolve(
          reads === 1
            ? reply(503, {
                error: {
                  code: 'unavailable',
                  message: 'the certificate store did not open',
                },
              })
            : reply(200, { clients: [], exclude_domains: [] }),
        );
      },
    });
    feed.deliver(rejection({ domain: 'api.bank.example' }));
    expect(groupRows(feed)).toHaveLength(1);
    expect(feed.dom.textContent).toContain('the certificate store did not open');
    expect(feed.button('Exclude').disabled).toBe(true);

    feed.click('Retry');
    await settle();
    expect(feed.dom.textContent).not.toContain('the certificate store did not open');
    expect(feed.button('Exclude').disabled).toBe(false);
    expect(feed.requests()).toBe(4);
    feed.release();
  });

  it('states the cap when the list is full', async () => {
    const feed = await open({
      answer: answered(
        { clients: [], exclude_domains: [] },
        {
          status: 422,
          body: {
            error: {
              code: 'validation_failed',
              message: 'exclude_domains: 513 entries exceed the cap of 512 by 1',
              details: {
                reason: 'over_cap',
                list: 'exclude_domains',
                len: 513,
                cap: 512,
              },
            },
          },
        },
      ),
    });
    feed.deliver(rejection({ domain: 'api.bank.example' }));
    feed.click('Exclude');
    feed.click('Exclude this host');
    await settle();

    const row = groupRows(feed)[0] ?? '';
    expect(row).toContain('513');
    expect(row).toContain('512');
    feed.release();
  });

  it('shows the API message when the store is unavailable', async () => {
    const feed = await open({
      answer: answered(
        { clients: [], exclude_domains: [] },
        {
          status: 503,
          body: {
            error: {
              code: 'unavailable',
              message: 'the certificate store did not open',
            },
          },
        },
      ),
    });
    feed.deliver(rejection({ domain: 'api.bank.example' }));
    feed.click('Exclude');
    feed.click('Exclude this host');
    await settle();

    expect(groupRows(feed)[0]).toContain('the certificate store did not open');
    feed.release();
  });

  it('freezes on pause and catches up on resume, as the rows view does', async () => {
    const feed = await open({
      answer: answered({ clients: [], exclude_domains: [] }),
    });
    feed.deliver(rejection({ domain: 'api.bank.example' }));
    feed.click('Pause');
    feed.deliver(
      rejection({ domain: 'api.bank.example' }),
      rejection({ domain: 'other.example' }),
    );

    expect(groupRows(feed)).toHaveLength(1);
    feed.click('Resume');
    expect(groupRows(feed)).toHaveLength(2);
    feed.release();
  });

  it('reads the document once and never polls', async () => {
    vi.useFakeTimers();
    try {
      const feed = await open({
        answer: answered({ clients: [], exclude_domains: [] }),
      });
      feed.deliver(rejection({ domain: 'api.bank.example' }));
      const before = feed.subscriptions();
      expect(feed.requests()).toBe(2);

      await act(async () => {
        vi.advanceTimersByTime(60_000);
        await Promise.resolve();
      });

      expect(feed.requests()).toBe(2);
      expect(feed.subscriptions()).toBe(before);
      feed.release();
    } finally {
      vi.useRealTimers();
    }
  });

  it('hides the filters, which have nothing to narrow here', async () => {
    const feed = await open({
      answer: answered({ clients: [], exclude_domains: [] }),
    });
    const chips = [...feed.dom.querySelectorAll('.feed-chipset .chip')].map(
      (node) => node.textContent,
    );
    expect(chips).toEqual(['all traffic', 'Certificate rejected by client']);
    expect(feed.dom.querySelector('#feed-domain')).toBeNull();
    feed.release();
  });
});
