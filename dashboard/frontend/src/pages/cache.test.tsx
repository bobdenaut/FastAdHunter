// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CacheUsage, Telemetry } from '../api/types';
import { RefreshRegistry } from '../refresh/registry';
import { BoundsCard } from './cache/bounds-card';
import { CleanCard } from './cache/clean-card';
import { CleanupCard } from './cache/cleanup-card';
import { CountersCard } from './cache/counters-card';
import { StageCard } from './cache/stage-card';
import { SwrCard } from './cache/swr-card';

/**
 * `Cache.dc.html` decides the wording and which figure sits where; API.md
 * decides what may be rendered at all. The cards are mounted directly rather
 * than through the page, exactly as `p5-06` mounts the Dashboard's — the page
 * itself is two `useRefresh` calls and a layout, and its route declaration is
 * pinned in `routes.test.ts`.
 */

const CACHE: CacheUsage = {
  entries: 1108,
  capacity: 50_000,
  fresh: 155,
  stale: 953,
  expired: 0,
  hits: 10_021,
  misses: 1150,
  evictions: 0,
  bytes: 1_153_433,
  max_bytes: 67_108_864,
  load_percent: 2.22,
  byte_load_percent: 1.72,
};

const TELEMETRY = {
  counters: {
    swr: {
      enqueued: 12_044,
      deduplicated: 3311,
      dropped: 0,
      completed: 8702,
      failed: 31,
    },
    cache_cleanup: {
      runs: 308,
      entries_removed: 44_120,
      bytes_freed: 9_871_232,
      last_duration_micros: 1842,
    },
  },
} as unknown as Telemetry;

const CLEAN = {
  removed_expired: 1834,
  removed_stale: 0,
  entries_before: 9095,
  entries_after: 7261,
  freed_bytes: 2_846_720,
  duration_ms: 4.7,
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

const fetchMock = vi.fn();
const store = new Map<string, string>();
let host: HTMLElement | null = null;
let registry: RefreshRegistry | null = null;

/** Stub fetchers, so a mounted `RefreshCluster` cannot reach the network. */
function stubRegistry(): RefreshRegistry {
  const idle = () => Promise.resolve({});
  registry = new RefreshRegistry({
    health: idle,
    telemetry: idle,
    cache: idle,
    clients: idle,
    lists: idle,
  });
  return registry;
}

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
    for (let turn = 0; turn < 8; turn += 1) await Promise.resolve();
  });
}

function unmount(): void {
  if (host === null) return;
  act(() => {
    render(null, host as HTMLElement);
  });
  host.remove();
  host = null;
}

beforeEach(() => {
  store.clear();
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => store.set(key, value),
  });
});

afterEach(() => {
  unmount();
  registry?.dispose();
  registry = null;
  vi.unstubAllGlobals();
});

describe('the lifetime stage bar', () => {
  it('draws the four bands and states what each one means', () => {
    const dom = mount(<StageCard cache={CACHE} />);
    const names = [...dom.querySelectorAll('.stage-name')].map((node) =>
      node.textContent?.replace(/\s+/g, ' ').trim(),
    );
    expect(names).toEqual([
      'fresh 155',
      'stale 953',
      'expired 0',
      // E1 — `capacity − entries`, and the only derived band on the card.
      'free 48,892',
    ]);
    expect(dom.textContent).toContain('outage insurance, not waste');
  });

  it('prints the entries-of-capacity secondary the artboard draws', () => {
    const dom = mount(<StageCard cache={CACHE} />);
    expect(dom.querySelector('.ch-right')?.textContent).toContain(
      '1,108 of 50,000',
    );
  });

  it('carries no latency figure — there is no per-stage source for one', () => {
    // The artboard prints `0.051 ms` inside the fresh note. The only same-page
    // source would be `telemetry.latency`'s lifetime mean, which API.md warns
    // is meaningless and which this task forbids everywhere.
    const dom = mount(<StageCard cache={CACHE} />);
    expect(dom.textContent).not.toContain('ms');
  });

  it('says so rather than drawing zeros before the first read', () => {
    const dom = mount(<StageCard cache={null} />);
    expect(dom.textContent).toContain('Not read yet');
  });
});

describe('the two bounds', () => {
  it('names whichever bound will evict first', () => {
    const dom = mount(<BoundsCard cache={CACHE} />);
    expect(dom.querySelector('.callout')?.textContent).toContain(
      'Entries is the bound closest to evicting',
    );
  });

  it('names bytes when the byte load is the higher one', () => {
    const dom = mount(
      <BoundsCard
        cache={{ ...CACHE, load_percent: 1.7, byte_load_percent: 2.2 }}
      />,
    );
    expect(dom.querySelector('.callout')?.textContent).toContain(
      'Bytes is the bound closest to evicting',
    );
  });

  it('claims neither when they are level', () => {
    const dom = mount(
      <BoundsCard
        cache={{ ...CACHE, load_percent: 2.2, byte_load_percent: 2.2 }}
      />,
    );
    expect(dom.querySelector('.callout')?.textContent).toContain(
      'Both bounds are equally loaded',
    );
  });

  it('renders both percentages verbatim and both bounds in MiB', () => {
    const dom = mount(<BoundsCard cache={CACHE} />);
    const text = dom.textContent ?? '';
    expect(text).toContain('2.2 %');
    expect(text).toContain('1.7 %');
    expect(text).toContain('1.1 MiB / 64 MiB');
    expect(text).toContain('coarse per-entry estimate');
  });
});

describe('the lifetime counters', () => {
  it('derives lookups and the hit rate and nothing else', () => {
    const dom = mount(<CountersCard cache={CACHE} />);
    const text = dom.textContent ?? '';
    expect(text).toContain('11,171');
    expect(text).toContain('89.7%');
  });

  it('draws the empty ring rather than a NaN on an untouched cache', () => {
    const dom = mount(
      <CountersCard cache={{ ...CACHE, hits: 0, misses: 0, evictions: 0 }} />,
    );
    expect(dom.textContent).toContain('0.0%');
    expect(dom.textContent).not.toContain('NaN');
  });
});

describe('the background-cleanup panel', () => {
  it('renders the last-value gauge as one current figure', () => {
    const dom = mount(<CleanupCard telemetry={TELEMETRY} />);
    expect(dom.textContent).toContain('1.84 ms');
    expect(dom.textContent).toContain('9.4 MiB');
  });

  it('plots no series of it, because deltaing a gauge is nonsense', () => {
    // `last_duration_micros` is the one last-value gauge in a block of
    // cumulative counters (API.md §telemetry).
    const dom = mount(<CleanupCard telemetry={TELEMETRY} />);
    expect(dom.querySelector('.chart')).toBeNull();
    expect(dom.querySelector('svg')).toBeNull();
  });
});

describe('the stale-while-revalidate panel', () => {
  it('renders the five counters verbatim', () => {
    const dom = mount(
      <SwrCard telemetry={TELEMETRY} registry={stubRegistry()} />,
    );
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    for (const figure of ['12,044', '3,311', '8,702', '31', '0']) {
      expect(text).toContain(figure);
    }
    expect(text).toContain('Dropped is the one to watch');
  });
});

describe('the clean action', () => {
  it('leaves the stale purge off on mount and posts without the parameter', async () => {
    fetchMock.mockResolvedValue(respond(200, CLEAN));
    const dom = mount(<CleanCard cache={CACHE} registry={stubRegistry()} />);
    const box = dom.querySelector('input[type=checkbox]') as HTMLInputElement;
    expect(box.checked).toBe(false);

    (dom.querySelector('.btn') as HTMLButtonElement).click();
    await flush();
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/cache/clean');
  });

  it('asks for the stale purge only once the operator chose it', async () => {
    fetchMock.mockResolvedValue(respond(200, CLEAN));
    const dom = mount(<CleanCard cache={CACHE} registry={stubRegistry()} />);
    const box = dom.querySelector('input[type=checkbox]') as HTMLInputElement;
    await act(async () => {
      box.checked = true;
      box.dispatchEvent(new Event('change', { bubbles: true }));
    });
    // Toggling the choice sends nothing on its own.
    expect(fetchMock).not.toHaveBeenCalled();

    (dom.querySelector('.btn') as HTMLButtonElement).click();
    await flush();
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/cache/clean?stale=true');
  });

  it('previews what a purge would remove, from the live snapshot', () => {
    const dom = mount(<CleanCard cache={CACHE} registry={stubRegistry()} />);
    expect((dom.textContent ?? '').replace(/\s+/g, ' ')).toContain(
      '(0 expired entries right now — this would remove 953 stale ones)',
    );
  });

  it('reports every response field, and what RSS will not do', async () => {
    fetchMock.mockResolvedValue(respond(200, CLEAN));
    const stub = stubRegistry();
    const invalidate = vi.spyOn(stub, 'invalidate');
    const dom = mount(<CleanCard cache={CACHE} registry={stub} />);
    (dom.querySelector('.btn') as HTMLButtonElement).click();
    await flush();

    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('1,834');
    expect(text).toContain('9,095 → 7,261');
    expect(text).toContain('2.7 MiB');
    expect(text).toContain('4.7 ms');
    expect(text).toContain('never shrinks the table slab');
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith('cache');
  });

  it('issues no cache re-read when the answer lands after the page has gone', async () => {
    // `registry.invalidate` fetches unconditionally, so firing it from a page
    // that has unmounted would put a `/cache` read on the log that no active
    // route owns — the exact activity the route-scoped invariant forbids.
    const held: { settle: (value: Response) => void } = {
      settle: () => undefined,
    };
    fetchMock.mockReturnValue(
      new Promise<Response>((resolve) => {
        held.settle = resolve;
      }),
    );
    const stub = stubRegistry();
    const invalidate = vi.spyOn(stub, 'invalidate');
    const dom = mount(<CleanCard cache={CACHE} registry={stub} />);
    (dom.querySelector('.btn') as HTMLButtonElement).click();
    await flush();

    unmount();
    held.settle(respond(200, CLEAN));
    await flush();
    expect(invalidate).not.toHaveBeenCalled();
  });

  it('renders the envelope’s own message when the clean is refused', async () => {
    fetchMock.mockResolvedValue(
      respond(503, {
        error: { code: 'unavailable', message: 'the cache is not running' },
      }),
    );
    const dom = mount(<CleanCard cache={CACHE} registry={stubRegistry()} />);
    (dom.querySelector('.btn') as HTMLButtonElement).click();
    await flush();

    expect(dom.querySelector('.error-state-message')?.textContent).toBe(
      'the cache is not running',
    );
    // The choice and the action survive a refusal.
    expect(dom.querySelector('.btn')?.textContent).toBe('Remove expired');
  });
});
