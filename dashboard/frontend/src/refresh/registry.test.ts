import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { REFRESH_DEFAULT_SECS } from '../constants';
import type { RefreshEndpoint } from '../router/routes';
import { RefreshRegistry, type EndpointState } from './registry';
import { writePreferredInterval } from './preferences';

const store = new Map<string, string>();

interface Counting {
  registry: RefreshRegistry;
  calls: Record<RefreshEndpoint, number>;
  resolve: (endpoint: RefreshEndpoint, value: unknown) => void;
  reject: (endpoint: RefreshEndpoint, error: Error) => void;
  settleAll: () => Promise<void>;
}

function counting(): Counting {
  const calls: Record<RefreshEndpoint, number> = {
    health: 0,
    telemetry: 0,
    cache: 0,
  };
  const pending = new Map<
    RefreshEndpoint,
    Array<{ resolve: (v: unknown) => void; reject: (e: Error) => void }>
  >();

  const fetcher = (endpoint: RefreshEndpoint) => () => {
    calls[endpoint] += 1;
    return new Promise<unknown>((resolve, reject) => {
      const queue = pending.get(endpoint) ?? [];
      queue.push({ resolve, reject });
      pending.set(endpoint, queue);
    });
  };

  const registry = new RefreshRegistry({
    health: fetcher('health'),
    telemetry: fetcher('telemetry'),
    cache: fetcher('cache'),
  });

  return {
    registry,
    calls,
    resolve: (endpoint, value) => {
      for (const entry of pending.get(endpoint) ?? []) entry.resolve(value);
      pending.set(endpoint, []);
    },
    reject: (endpoint, error) => {
      for (const entry of pending.get(endpoint) ?? []) entry.reject(error);
      pending.set(endpoint, []);
    },
    settleAll: async () => {
      await vi.advanceTimersByTimeAsync(0);
    },
  };
}

beforeEach(() => {
  store.clear();
  vi.useFakeTimers();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => store.set(key, value),
  });
  vi.stubGlobal('window', {
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('sharing one request', () => {
  it('fetches for the first subscriber and starts one timer', async () => {
    const h = counting();
    h.registry.subscribe('telemetry', () => {});
    expect(h.calls.telemetry).toBe(1);
    expect(h.registry.activeTimers()).toBe(1);
    h.registry.dispose();
  });

  it('serves a later subscriber the retained value and issues no request', async () => {
    const h = counting();
    h.registry.subscribe('telemetry', () => {});
    h.resolve('telemetry', { rules: 1 });
    await h.settleAll();

    const seen: EndpointState[] = [];
    h.registry.subscribe('telemetry', (state) => seen.push(state));
    expect(h.calls.telemetry).toBe(1);
    expect(seen[0]?.data).toEqual({ rules: 1 });
    h.registry.dispose();
  });

  it('shares one in-flight promise between concurrent callers', async () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    const a = h.registry.invalidate('cache');
    const b = h.registry.invalidate('cache');
    expect(h.calls.cache).toBe(1);
    h.resolve('cache', { entries: 2 });
    await Promise.all([a, b]);
    expect(h.calls.cache).toBe(1);
    h.registry.dispose();
  });

  it('gives two cards on one endpoint one request per interval', async () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    h.registry.subscribe('cache', () => {});
    expect(h.calls.cache).toBe(1);
    h.resolve('cache', {});
    await h.settleAll();

    vi.advanceTimersByTime(REFRESH_DEFAULT_SECS.cache * 1000);
    expect(h.calls.cache).toBe(2);
    h.registry.dispose();
  });
});

describe('shared, not global', () => {
  it('runs no timer and issues no request with no subscriber', () => {
    const h = counting();
    expect(h.registry.activeTimers()).toBe(0);
    vi.advanceTimersByTime(600_000);
    expect(h.calls.telemetry).toBe(0);
    expect(h.calls.health).toBe(0);
    expect(h.calls.cache).toBe(0);
    h.registry.dispose();
  });

  it('clears the timer on the last unsubscribe', () => {
    const h = counting();
    const first = h.registry.subscribe('health', () => {});
    const second = h.registry.subscribe('health', () => {});
    first();
    expect(h.registry.activeTimers()).toBe(1);
    second();
    expect(h.registry.activeTimers()).toBe(0);

    vi.advanceTimersByTime(600_000);
    expect(h.calls.health).toBe(1);
    h.registry.dispose();
  });

  it('ignores a repeated release', () => {
    const h = counting();
    const release = h.registry.subscribe('health', () => {});
    h.registry.subscribe('health', () => {});
    release();
    release();
    expect(h.registry.subscriberCount('health')).toBe(1);
    h.registry.dispose();
  });

  it('runs each endpoint at its own interval, not one shared period', async () => {
    const h = counting();
    writePreferredInterval('health', 30);
    h.registry.subscribe('health', () => {});
    h.registry.subscribe('telemetry', () => {});
    h.resolve('health', {});
    h.resolve('telemetry', {});
    await h.settleAll();

    vi.advanceTimersByTime(30_000);
    expect(h.calls.health).toBe(2);
    expect(h.calls.telemetry).toBe(1);
    h.registry.dispose();
  });
});

describe('the retained value', () => {
  it('survives the last unsubscribe while the timer does not', async () => {
    const h = counting();
    const release = h.registry.subscribe('cache', () => {});
    h.resolve('cache', { entries: 9 });
    await h.settleAll();
    release();

    expect(h.registry.activeTimers()).toBe(0);
    expect(h.registry.retained('cache').data).toEqual({ entries: 9 });
    h.registry.dispose();
  });

  it('is served immediately on the next subscribe and then revalidated', async () => {
    const h = counting();
    const release = h.registry.subscribe('cache', () => {});
    h.resolve('cache', { entries: 9 });
    await h.settleAll();
    release();

    const seen: EndpointState[] = [];
    h.registry.subscribe('cache', (state) => seen.push(state));
    expect(seen[0]?.data).toEqual({ entries: 9 });
    expect(h.calls.cache).toBe(2);
    h.registry.dispose();
  });

  it('holds at most one value per endpoint', async () => {
    const h = counting();
    for (const endpoint of ['health', 'telemetry', 'cache'] as const) {
      const release = h.registry.subscribe(endpoint, () => {});
      h.resolve(endpoint, { seen: endpoint });
      await h.settleAll();
      release();
    }
    expect(h.registry.retained('health').data).toEqual({ seen: 'health' });
    expect(h.registry.retained('telemetry').data).toEqual({
      seen: 'telemetry',
    });
    expect(h.registry.retained('cache').data).toEqual({ seen: 'cache' });
    h.registry.dispose();
  });
});

describe('failure', () => {
  it('keeps the previous value and leaves the timer running', async () => {
    const h = counting();
    const seen: EndpointState[] = [];
    h.registry.subscribe('cache', (state) => seen.push(state));
    h.resolve('cache', { entries: 1 });
    await h.settleAll();

    vi.advanceTimersByTime(REFRESH_DEFAULT_SECS.cache * 1000);
    h.reject('cache', new Error('upstream is unhappy'));
    await h.settleAll();

    const last = seen[seen.length - 1];
    expect(last?.data).toEqual({ entries: 1 });
    expect(last?.error?.message).toBe('upstream is unhappy');
    expect(h.registry.activeTimers()).toBe(1);
    h.registry.dispose();
  });
});

describe('manual refresh', () => {
  it('updates every card on that endpoint from one request', async () => {
    const h = counting();
    const a: EndpointState[] = [];
    const b: EndpointState[] = [];
    h.registry.subscribe('cache', (state) => a.push(state));
    h.registry.subscribe('cache', (state) => b.push(state));
    h.resolve('cache', { entries: 1 });
    await h.settleAll();

    const invalidated = h.registry.invalidate('cache');
    h.resolve('cache', { entries: 2 });
    await invalidated;
    await h.settleAll();

    expect(h.calls.cache).toBe(2);
    expect(a[a.length - 1]?.data).toEqual({ entries: 2 });
    expect(b[b.length - 1]?.data).toEqual({ entries: 2 });
    h.registry.dispose();
  });

  it('joins an in-flight request rather than queueing a second', async () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    void h.registry.invalidate('cache');
    void h.registry.invalidate('cache');
    void h.registry.invalidate('cache');
    expect(h.calls.cache).toBe(1);
    h.registry.dispose();
  });

  it('restarts the background timer from the manual fetch', async () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    h.resolve('cache', {});
    await h.settleAll();

    vi.advanceTimersByTime(REFRESH_DEFAULT_SECS.cache * 1000 - 5_000);
    const manual = h.registry.invalidate('cache');
    h.resolve('cache', {});
    await manual;
    await h.settleAll();
    expect(h.calls.cache).toBe(2);

    vi.advanceTimersByTime(6_000);
    expect(h.calls.cache).toBe(2);
    vi.advanceTimersByTime(REFRESH_DEFAULT_SECS.cache * 1000);
    expect(h.calls.cache).toBe(3);
    h.registry.dispose();
  });

  it('restarts the background timer even when the click joined a background fetch', async () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    h.resolve('cache', {});
    await h.settleAll();
    expect(h.calls.cache).toBe(1);

    // The scheduled fetch lands, and the operator clicks Refresh while it is
    // still in flight. The click joins it — and still owns the timer reset.
    vi.advanceTimersByTime(REFRESH_DEFAULT_SECS.cache * 1000);
    expect(h.calls.cache).toBe(2);
    const manual = h.registry.invalidate('cache');
    expect(h.calls.cache).toBe(2);
    h.resolve('cache', {});
    await manual;
    await h.settleAll();

    // Restarted from the fetch that answered the click, not left on the
    // original schedule.
    vi.advanceTimersByTime(REFRESH_DEFAULT_SECS.cache * 1000 - 1_000);
    expect(h.calls.cache).toBe(2);
    vi.advanceTimersByTime(2_000);
    expect(h.calls.cache).toBe(3);
    h.registry.dispose();
  });

  it('leaves the timer alone when the joined request failed', async () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    h.resolve('cache', {});
    await h.settleAll();

    vi.advanceTimersByTime(REFRESH_DEFAULT_SECS.cache * 1000);
    const manual = h.registry.invalidate('cache');
    h.reject('cache', new Error('nope'));
    await manual;
    await h.settleAll();
    expect(h.calls.cache).toBe(2);

    // The next scheduled read still arrives on the original cadence, and the
    // flag did not survive to arm it.
    vi.advanceTimersByTime(REFRESH_DEFAULT_SECS.cache * 1000);
    expect(h.calls.cache).toBe(3);
    h.registry.dispose();
  });

  it('works for an endpoint nothing is subscribed to without starting a timer', async () => {
    const h = counting();
    const invalidated = h.registry.invalidate('health');
    h.resolve('health', { status: 'ok' });
    await invalidated;
    expect(h.calls.health).toBe(1);
    expect(h.registry.activeTimers()).toBe(0);
    h.registry.dispose();
  });
});

describe('the interval selector', () => {
  it('rebuilds a live timer from now and issues no request', async () => {
    const h = counting();
    h.registry.subscribe('telemetry', () => {});
    h.resolve('telemetry', {});
    await h.settleAll();
    expect(h.calls.telemetry).toBe(1);

    vi.advanceTimersByTime(200_000);
    h.registry.setRefreshInterval('telemetry', 60);
    expect(h.calls.telemetry).toBe(1);

    // Elapsed time is discarded, not rebased: rebasing 300 s to 60 s after
    // 200 s elapsed would fire instantly.
    vi.advanceTimersByTime(59_000);
    expect(h.calls.telemetry).toBe(1);
    vi.advanceTimersByTime(2_000);
    expect(h.calls.telemetry).toBe(2);
    h.registry.dispose();
  });

  it('is a no-op with nothing subscribed', () => {
    const h = counting();
    expect(h.registry.setRefreshInterval('telemetry', 60)).toBe(true);
    expect(h.registry.activeTimers()).toBe(0);
    expect(h.calls.telemetry).toBe(0);
    h.registry.dispose();
  });

  it('refuses a value the endpoint does not offer', () => {
    const h = counting();
    expect(h.registry.setRefreshInterval('telemetry', 30)).toBe(false);
    h.registry.dispose();
  });
});

describe('suspension', () => {
  it('clears every timer and starts nothing while hidden', async () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    h.registry.subscribe('telemetry', () => {});
    h.resolve('cache', {});
    h.resolve('telemetry', {});
    await h.settleAll();

    h.registry.setSuspended(true);
    expect(h.registry.activeTimers()).toBe(0);
    vi.advanceTimersByTime(600_000);
    expect(h.calls.cache).toBe(1);
    expect(h.calls.telemetry).toBe(1);
    h.registry.dispose();
  });

  it('resumes and refetches only what is stale', async () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    h.registry.subscribe('health', () => {});
    h.resolve('cache', {});
    h.resolve('health', {});
    await h.settleAll();

    h.registry.setSuspended(true);
    // Past /health's 60 s, inside /cache's 300 s.
    vi.advanceTimersByTime(90_000);
    h.registry.setSuspended(false);

    expect(h.calls.health).toBe(2);
    expect(h.calls.cache).toBe(1);
    expect(h.registry.activeTimers()).toBe(2);
    h.registry.dispose();
  });

  it('subscribes without fetching while suspended', () => {
    const h = counting();
    h.registry.setSuspended(true);
    h.registry.subscribe('cache', () => {});
    expect(h.calls.cache).toBe(0);
    expect(h.registry.activeTimers()).toBe(0);
    h.registry.dispose();
  });
});
