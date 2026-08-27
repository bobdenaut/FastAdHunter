import { describe, expect, it } from 'vitest';
import {
  EVENT_TYPES,
  GALLERY_ROUTE,
  LOGIN_ROUTE,
  REFRESH_ENDPOINTS,
  ROUTES,
  SECTION_LABELS,
  effectiveEndpoints,
  effectiveEvents,
} from './routes';

describe('the route table', () => {
  it('ships all thirteen product screens', () => {
    expect(ROUTES).toHaveLength(13);
    expect(new Set(ROUTES.map((r) => r.path)).size).toBe(13);
  });

  it('declares only the four documented event names', () => {
    for (const route of [...ROUTES, LOGIN_ROUTE, GALLERY_ROUTE]) {
      for (const type of route.events) {
        expect(EVENT_TYPES).toContain(type);
      }
    }
  });

  it('declares only the polled endpoints the registry knows', () => {
    for (const route of [...ROUTES, LOGIN_ROUTE, GALLERY_ROUTE]) {
      for (const endpoint of route.endpoints) {
        expect(REFRESH_ENDPOINTS).toContain(endpoint);
      }
    }
  });

  // Pinned, not merely typed: the union is what `useRefresh`, `RefreshCluster`,
  // `preferences.ts` and `registry.ts` are all keyed on, so a sixth polled
  // endpoint has to be an edit somebody made on purpose.
  it('polls exactly these five endpoints and no others', () => {
    expect([...REFRESH_ENDPOINTS]).toEqual([
      'health',
      'telemetry',
      'cache',
      'clients',
      'lists',
    ]);
  });

  it('puts `query` on exactly one screen, and it is the Live Feed', () => {
    const withQuery = ROUTES.filter((r) => r.events.includes('query'));
    expect(withQuery.map((r) => r.path)).toEqual(['/diagnostics/live-feed']);
  });

  it('gives the Dashboard the stats push and the five polled endpoints', () => {
    const dashboard = ROUTES.find((r) => r.path === '/');
    expect(dashboard?.events).toEqual(['stats']);
    expect(dashboard?.endpoints).toEqual([
      'telemetry',
      'cache',
      'health',
      'clients',
      'lists',
    ]);
  });

  // The SWR and background-cleanup panels are `counters.swr` and
  // `counters.cache_cleanup`, which live on `/telemetry` and nowhere else. Both
  // go through the one shared mechanism, so the page still starts no timer of
  // its own — and `REFRESH_ENDPOINTS` is unchanged by this, which the pin above
  // is what proves.
  it('gives Cache both endpoints its cards actually read', () => {
    const cache = ROUTES.find((r) => r.path === '/cache');
    expect(cache?.endpoints).toEqual(['cache', 'telemetry']);
    expect(cache?.events).toEqual([]);
  });

  it('polls nothing on Performance — /history/perf is a range query', () => {
    const performance = ROUTES.find((r) => r.path === '/performance');
    expect(performance?.endpoints).toEqual([]);
  });

  it('groups the three diagnostics screens and nothing else', () => {
    expect(ROUTES.filter((r) => r.group === 'diagnostics').map((r) => r.path))
      .toEqual([
        '/diagnostics/health',
        '/diagnostics/memory',
        '/diagnostics/live-feed',
      ]);
  });

  it('labels every section it uses', () => {
    for (const route of ROUTES) {
      expect(SECTION_LABELS[route.section]).toBeTruthy();
    }
  });
});

describe('what a route actually acquires', () => {
  it('is nothing at all while the screen is not built', () => {
    for (const route of ROUTES.filter((r) => !r.built)) {
      expect(effectiveEvents(route)).toEqual([]);
      expect(effectiveEndpoints(route)).toEqual([]);
    }
  });

  it('is the declaration for the screens built so far, and only those', () => {
    expect(ROUTES.filter((r) => r.built).map((r) => r.path)).toEqual([
      '/',
      '/lists',
      '/rules',
      '/policies',
      '/clients',
      '/rule-tester',
      '/cache',
      '/performance',
      '/upstreams',
    ]);
    const dashboard = ROUTES.find((r) => r.path === '/');
    expect(effectiveEvents(dashboard!)).toEqual(['stats']);
    expect(effectiveEndpoints(dashboard!)).toHaveLength(5);
  });

  // The compile duration is one field read once per mount, not a polled
  // endpoint — declaring `telemetry` here would put a timer on it.
  it('gives Lists the inventory and its event, and no `telemetry` poll', () => {
    const lists = ROUTES.find((r) => r.path === '/lists');
    expect(lists?.events).toEqual(['list_refreshed']);
    expect(lists?.endpoints).toEqual(['lists']);
  });

  it('is the declaration once the screen is built', () => {
    expect(effectiveEvents(GALLERY_ROUTE)).toEqual(['stats']);
    expect(effectiveEndpoints(GALLERY_ROUTE)).toEqual(['telemetry', 'cache']);
  });

  it('is nothing on the login route', () => {
    expect(effectiveEvents(LOGIN_ROUTE)).toEqual([]);
    expect(effectiveEndpoints(LOGIN_ROUTE)).toEqual([]);
  });
});

describe('lazy loading', () => {
  it('splits the Settings and Diagnostics group into its own chunk', () => {
    const system = ROUTES.filter((r) => r.section === 'system');
    expect(system).toHaveLength(4);
    for (const route of system) {
      expect(route.load).not.toBeNull();
    }
  });
});
