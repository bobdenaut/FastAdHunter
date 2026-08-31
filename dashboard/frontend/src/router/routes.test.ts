import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import {
  EVENT_TYPES,
  GALLERY_ROUTE,
  GROUP_LABELS,
  LOGIN_ROUTE,
  MOVED,
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

  it('maps every moved path to a route that exists', () => {
    expect(MOVED).toEqual({ '/diagnostics/live-feed': '/live-feed' });
    for (const target of Object.values(MOVED)) {
      expect(ROUTES.map((r) => r.path)).toContain(target);
    }
  });

  it('puts `query` on exactly one screen, and it is the Live Feed', () => {
    const withQuery = ROUTES.filter((r) => r.events.includes('query'));
    expect(withQuery.map((r) => r.path)).toEqual(['/live-feed']);
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

  it('groups the two diagnostics screens and nothing else', () => {
    expect(ROUTES.filter((r) => r.group === 'diagnostics').map((r) => r.path))
      .toEqual(['/diagnostics/health', '/diagnostics/memory']);
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
      '/live-feed',
      '/lists',
      '/rules',
      '/policies',
      '/clients',
      '/rule-tester',
      '/cache',
      '/performance',
      '/upstreams',
      '/settings',
      '/diagnostics/health',
      '/diagnostics/memory',
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
    expect(system).toHaveLength(3);
    for (const route of system) {
      expect(route.load).not.toBeNull();
    }
  });
});

describe('the System section’s declarations', () => {
  it('gives Health the three endpoints its cards read, and no event', () => {
    // The list-problem summary is `GET /lists`, which lives nowhere else; the
    // strategy is one `/config` read on mount, boot-only and never re-read.
    const health = ROUTES.find((r) => r.path === '/diagnostics/health');
    expect(health?.endpoints).toEqual(['health', 'telemetry', 'lists']);
    expect(health?.events).toEqual([]);
  });

  it('polls nothing on Memory — both of its reads are one-shots', () => {
    const memory = ROUTES.find((r) => r.path === '/diagnostics/memory');
    expect(memory?.endpoints).toEqual([]);
    expect(memory?.events).toEqual([]);
  });

  it('gives Settings its event and no polled endpoint', () => {
    // `/config` is an entry read plus one per `config_changed`; `/health` is
    // read on entry only while the restart banner is armed. Neither is a poll,
    // so declaring an endpoint here would put a timer on this page.
    const settings = ROUTES.find((r) => r.path === '/settings');
    expect(settings?.events).toEqual(['config_changed']);
    expect(settings?.endpoints).toEqual([]);
    expect(settings?.built).toBe(true);
    expect(settings?.ownsHeader).toBe(true);
  });
});

describe('the System section, now that all four ship', () => {
  it('builds every one of them and lets each own its header', () => {
    const system = ROUTES.filter((r) => r.section === 'system');
    expect(system.map((r) => r.path)).toEqual([
      '/settings',
      '/diagnostics/health',
      '/diagnostics/memory',
    ]);
    for (const route of system) {
      expect(route.built, route.path).toBe(true);
      expect(route.ownsHeader, route.path).toBe(true);
    }
  });

  it('leaves no route on the not-yet-built placeholder', () => {
    // `pages/system.tsx` is deleted with its last consumer: every one of the
    // thirteen screens is a page now.
    expect(ROUTES.filter((r) => !r.built)).toEqual([]);
  });

  it('gives the Live Feed the query subscription and nothing polled', () => {
    const feed = ROUTES.find((r) => r.path === '/live-feed');
    expect(feed?.events).toEqual(['query']);
    expect(feed?.endpoints).toEqual([]);
  });
});

describe('the nested group label', () => {
  it('names every group a route declares', () => {
    // The top bar built its prefix from a literal `'diagnostics' → 'Diagnostics'`
    // conditional, so a second nested group would have lost its prefix in
    // silence. The label now comes from a `Record` over the union: adding a
    // group fails to compile until it is named, and this asserts the table is
    // complete for the routes that exist.
    for (const route of ROUTES) {
      if (route.group === undefined) continue;
      expect(GROUP_LABELS[route.group], route.path).toBeTruthy();
    }
    expect(Object.keys(GROUP_LABELS)).toEqual(['diagnostics']);
  });

  it('gives every group a sprite symbol, because the key is the glyph name', () => {
    // The sidebar draws `<Icon name={group} />`, so a group without a symbol
    // ships a blank tile beside its label. The sidebar no longer names a group
    // in its own source — this is what the generic block rests on.
    const sprite = readFileSync(
      fileURLToPath(new URL('../assets/sprite.svg', import.meta.url)),
      'utf8',
    );
    for (const group of Object.keys(GROUP_LABELS)) {
      expect(sprite, group).toContain(`id="${group}"`);
    }
  });

  it('keeps every group’s routes in one section, which is where it is drawn', () => {
    // The sidebar places a group under the section of its first route. Members
    // spread across two sections would silently draw the whole group under the
    // first one's.
    const sections = new Map<string, Set<string>>();
    for (const route of ROUTES) {
      if (route.group === undefined) continue;
      const seen = sections.get(route.group) ?? new Set<string>();
      sections.set(route.group, seen.add(route.section));
    }
    for (const [group, seen] of sections) expect(seen.size, group).toBe(1);
  });
});
