import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { HIDDEN_CLOSE_GRACE_MS } from '../constants';
import { SocketManager, type SocketLike } from '../events/socket';
import { SubscriptionRegistry } from '../events/subscriptions';
import { RefreshRegistry } from '../refresh/registry';
import type { Route } from '../router/routes';
import { RouteLifecycle } from './route-lifecycle';

class FakeSocket implements SocketLike {
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  closedWith: number | null = null;
  readonly sent: string[] = [];
  send(data: string): void {
    this.sent.push(data);
  }
  close(code?: number): void {
    this.closedWith = code ?? 1000;
  }
}

function route(over: Partial<Route>): Route {
  return {
    path: '/x',
    title: 'X',
    section: 'runtime',
    events: [],
    endpoints: [],
    built: true,
    load: null,
    ...over,
  };
}

interface Harness {
  lifecycle: RouteLifecycle;
  sockets: FakeSocket[];
  calls: Record<string, number>;
  refresh: RefreshRegistry;
  subscriptions: SubscriptionRegistry;
  socket: SocketManager;
}

function harness(assert = true): Harness {
  const sockets: FakeSocket[] = [];
  const calls: Record<string, number> = { health: 0, telemetry: 0, cache: 0 };
  const fetcher = (name: string) => () => {
    calls[name] = (calls[name] ?? 0) + 1;
    return Promise.resolve({});
  };
  const subscriptions = new SubscriptionRegistry();
  const refresh = new RefreshRegistry({
    health: fetcher('health'),
    telemetry: fetcher('telemetry'),
    cache: fetcher('cache'),
  });
  const socket = new SocketManager({
    subscriptions,
    url: 'wss://box/api/v1/events',
    open: () => {
      const next = new FakeSocket();
      sockets.push(next);
      return next;
    },
    onAuthFailure: () => {},
    probe: () => Promise.resolve('inconclusive'),
    random: () => 0.5,
  });
  const lifecycle = new RouteLifecycle({
    subscriptions,
    refresh,
    socket,
    assert,
  });
  return { lifecycle, sockets, calls, refresh, subscriptions, socket };
}

const store = new Map<string, string>();

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

describe('the route transition', () => {
  it('acquires what the incoming route declares and nothing else', () => {
    const h = harness();
    h.lifecycle.enter(route({ events: ['stats'], endpoints: ['telemetry'] }));
    expect(h.subscriptions.union()).toEqual(['stats']);
    expect(h.refresh.subscriberCount('telemetry')).toBe(1);
    expect(h.refresh.subscriberCount('cache')).toBe(0);
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('stops the outgoing route’s work', () => {
    const h = harness();
    h.lifecycle.enter(route({ events: ['stats'], endpoints: ['telemetry'] }));
    h.lifecycle.enter(route({ path: '/y' }));
    expect(h.subscriptions.union()).toEqual([]);
    expect(h.refresh.activeTimers()).toBe(0);
    vi.advanceTimersByTime(600_000);
    expect(h.calls['telemetry']).toBe(1);
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('acquires nothing for a route whose screen is not built', () => {
    const h = harness();
    h.lifecycle.enter(
      route({ events: ['stats'], endpoints: ['telemetry'], built: false }),
    );
    expect(h.subscriptions.union()).toEqual([]);
    expect(h.refresh.activeTimers()).toBe(0);
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('closes the socket when the incoming route needs no events', () => {
    const h = harness();
    h.lifecycle.enter(route({ events: ['stats'] }));
    const socket = h.sockets[0];
    socket?.onopen?.();
    expect(h.socket.indicator()).toBe('live');

    h.lifecycle.enter(route({ path: '/y' }));
    expect(socket?.closedWith).toBe(1000);
    expect(h.socket.indicator()).toBe('not-needed-here');
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('does not rebuild the socket between two routes that both want stats', () => {
    const h = harness();
    h.lifecycle.enter(route({ events: ['stats'] }));
    const socket = h.sockets[0];
    socket?.onopen?.();
    h.lifecycle.enter(route({ path: '/y', events: ['stats'] }));
    expect(h.sockets).toHaveLength(1);
    expect(socket?.closedWith).toBeNull();
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('leaves everything released when it enters nothing', () => {
    const h = harness();
    h.lifecycle.enter(route({ events: ['query'], endpoints: ['cache'] }));
    h.lifecycle.enter(null);
    expect(h.subscriptions.union()).toEqual([]);
    expect(h.refresh.subscriberCount('cache')).toBe(0);
    h.refresh.dispose();
    h.socket.dispose();
  });
});

describe('the dev-mode assertion', () => {
  it('passes when the union matches the declaration', () => {
    const h = harness(true);
    expect(() =>
      h.lifecycle.enter(route({ events: ['stats', 'query'] })),
    ).not.toThrow();
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('reports a subscription the incoming route did not declare', () => {
    const h = harness(true);
    // A page that acquired on its own — the thing the shell-only rule forbids.
    h.subscriptions.acquire(['query']);
    expect(() => h.lifecycle.enter(route({ events: ['stats'] }))).toThrow(
      /does not match declaration/,
    );
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('reports an endpoint the transition failed to release', () => {
    const h = harness(true);
    h.lifecycle.enter(route({ path: '/a', endpoints: ['cache'] }));
    // The outgoing release never runs — the leak the net exists to catch.
    const leaked = h.lifecycle as unknown as { releases: Array<() => void> };
    leaked.releases = [];
    expect(() => h.lifecycle.enter(route({ path: '/b' }))).toThrow(
      /cache held=1 declared=0/,
    );
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('reports an endpoint released twice', () => {
    const h = harness(true);
    h.lifecycle.enter(route({ path: '/a', endpoints: ['cache'] }));
    const doubled = h.lifecycle as unknown as { releases: Array<() => void> };
    doubled.releases = [...doubled.releases, ...doubled.releases];
    expect(() =>
      h.lifecycle.enter(route({ path: '/b', endpoints: ['cache'] })),
    ).toThrow(/cache held=0 declared=1/);
    h.refresh.dispose();
    h.socket.dispose();
  });

  /**
   * The shell renders the previous page component until a later effect swaps
   * it, so a widget on the outgoing page is still subscribed when the
   * transition runs. Reading the registry's global count reported every
   * ordinary navigation between two built pages as a leak — and a net that
   * indicts the common case gets deleted rather than fixed.
   */
  it('does not indict a widget of the outgoing page that has yet to unmount', () => {
    const h = harness(true);
    h.lifecycle.enter(route({ path: '/a', endpoints: ['telemetry'] }));
    // What `useRefresh` does from inside the still-mounted outgoing page.
    const widget = h.refresh.subscribe('telemetry', () => {});
    expect(() =>
      h.lifecycle.enter(route({ path: '/b', endpoints: ['cache'] })),
    ).not.toThrow();
    // And the endpoint really does stop once that page unmounts a tick later.
    widget();
    expect(h.refresh.subscriberCount('telemetry')).toBe(0);
    expect(h.refresh.activeTimers()).toBe(1);
    h.refresh.dispose();
    h.socket.dispose();
  });
});

describe('suspension', () => {
  it('stops polling at once and closes the socket after the grace', () => {
    const h = harness();
    h.lifecycle.enter(route({ events: ['stats'], endpoints: ['cache'] }));
    const socket = h.sockets[0];
    socket?.onopen?.();

    h.lifecycle.setSuspended(true);
    expect(h.refresh.activeTimers()).toBe(0);
    expect(socket?.closedWith).toBeNull();

    vi.advanceTimersByTime(HIDDEN_CLOSE_GRACE_MS + 1);
    expect(socket?.closedWith).toBe(1000);
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('produces no reconnect on a hide/show inside the grace', () => {
    const h = harness();
    h.lifecycle.enter(route({ events: ['stats'] }));
    const socket = h.sockets[0];
    socket?.onopen?.();

    h.lifecycle.setSuspended(true);
    vi.advanceTimersByTime(HIDDEN_CLOSE_GRACE_MS / 2);
    h.lifecycle.setSuspended(false);
    vi.advanceTimersByTime(HIDDEN_CLOSE_GRACE_MS * 2);

    expect(h.sockets).toHaveLength(1);
    expect(socket?.closedWith).toBeNull();
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('opens no connection for a route entered while suspended', () => {
    const h = harness();
    h.lifecycle.setSuspended(true);
    h.lifecycle.enter(route({ events: ['stats'] }));
    vi.advanceTimersByTime(600_000);
    expect(h.sockets).toHaveLength(0);
    h.refresh.dispose();
    h.socket.dispose();
  });

  it('restores exactly the active route’s subscriptions on becoming visible', () => {
    const h = harness();
    h.lifecycle.enter(route({ events: ['stats'] }));
    h.sockets[0]?.onopen?.();
    h.lifecycle.setSuspended(true);
    vi.advanceTimersByTime(HIDDEN_CLOSE_GRACE_MS + 1);

    h.lifecycle.enter(route({ path: '/y', events: ['query'] }));
    h.lifecycle.setSuspended(false);

    const reopened = h.sockets[h.sockets.length - 1];
    reopened?.onopen?.();
    expect(reopened?.sent).toEqual(['{"subscribe":["query"]}']);
    h.refresh.dispose();
    h.socket.dispose();
  });
});
