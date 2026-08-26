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

  it('declares only the three polled endpoints', () => {
    for (const route of [...ROUTES, LOGIN_ROUTE, GALLERY_ROUTE]) {
      for (const endpoint of route.endpoints) {
        expect(REFRESH_ENDPOINTS).toContain(endpoint);
      }
    }
  });

  it('puts `query` on exactly one screen, and it is the Live Feed', () => {
    const withQuery = ROUTES.filter((r) => r.events.includes('query'));
    expect(withQuery.map((r) => r.path)).toEqual(['/diagnostics/live-feed']);
  });

  it('gives the Dashboard the stats push and no /health poll', () => {
    const dashboard = ROUTES.find((r) => r.path === '/');
    expect(dashboard?.events).toEqual(['stats']);
    expect(dashboard?.endpoints).toEqual(['telemetry', 'cache']);
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
    for (const route of ROUTES) {
      expect(route.built).toBe(false);
      expect(effectiveEvents(route)).toEqual([]);
      expect(effectiveEndpoints(route)).toEqual([]);
    }
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
