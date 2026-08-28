import type { ComponentType } from 'preact';
import { EVENT_TYPES, type EventType } from '../events/types';

/**
 * The five endpoints the shared bounded refresh reads. Nothing pushed is
 * polled: `stats` arrives on the socket and is never in this set.
 *
 * `clients` and `lists` joined the set in p5-06. A Top-clients table frozen at
 * page-entry beside tiles that move every two seconds is quiet wrongness, and
 * the alternative to polling them is a figure that silently goes stale — but
 * they go through this one mechanism and no other, so they inherit its
 * refcounting, its last-unsubscribe teardown and its suspend. A sixth name is
 * a deliberate edit: `routes.test.ts` pins the list.
 */
export const REFRESH_ENDPOINTS = [
  'health',
  'telemetry',
  'cache',
  'clients',
  'lists',
] as const;
export type RefreshEndpoint = (typeof REFRESH_ENDPOINTS)[number];

export interface PageProps {
  route: Route;
}

export { EVENT_TYPES };
export type { EventType };

export type Section = 'overview' | 'filtering' | 'runtime' | 'system';

/** The nested groups, and the one place their display name is written. */
export type RouteGroup = 'diagnostics';

/**
 * The label the sidebar and the top bar both draw for a group.
 *
 * A `Record` over the union rather than a literal at each site: adding a second
 * group now fails to compile until it is named here, where the top bar's
 * `'diagnostics' → 'Diagnostics'` conditional would have dropped its prefix in
 * silence.
 */
export const GROUP_LABELS: Record<RouteGroup, string> = {
  diagnostics: 'Diagnostics',
};

export interface Route {
  path: string;
  title: string;
  section: Section;
  group?: RouteGroup;
  /** Acquired on mount, released on unmount — by the shell's route transition
   *  and by nothing else. */
  events: readonly EventType[];
  endpoints: readonly RefreshEndpoint[];
  /**
   * `false` while the screen is the not-yet-built empty state. An unbuilt route
   * renders no events and no figures, so it must hold no subscription — the
   * columns below are what the row declares once the task that owns it lands.
   */
  built: boolean;
  /**
   * The page renders its own `<ContentHeader>` and the shell renders none.
   * Declared here rather than signalled from the page: the shell's header is
   * painted before the lazy chunk resolves, so a page-side flag would flash a
   * duplicate title on every entry, and a shared "the page took the header"
   * flag would be exactly the stale closure this table exists to avoid.
   */
  ownsHeader?: boolean;
  load: (() => Promise<{ default: ComponentType<PageProps> }>) | null;
}

/**
 * The single declarative source. A page declares nothing about its own data
 * lifecycle; the shell reads this table.
 */
export const ROUTES: readonly Route[] = [
  {
    path: '/',
    title: 'Dashboard',
    section: 'overview',
    // `stats` only. The page renders nothing a `config_changed` or a
    // `list_refreshed` event would change, and it renders no per-query rows —
    // so it must not receive `query` either.
    events: ['stats'],
    // `health` for the Uptime tile's "status ok" footer; `clients` and `lists`
    // because their cards would otherwise be frozen at page-entry beside tiles
    // that move every two seconds (D1a).
    endpoints: ['telemetry', 'cache', 'health', 'clients', 'lists'],
    built: true,
    load: () => import('../pages/dashboard'),
  },
  {
    path: '/lists',
    title: 'Lists',
    section: 'filtering',
    events: ['list_refreshed'],
    // The inventory only. `/telemetry` is read once on mount for the compile
    // duration and is deliberately **not** declared: declaring it would put a
    // standing timer on an endpoint this page renders one field of.
    endpoints: ['lists'],
    built: true,
    load: () => import('../pages/lists'),
  },
  {
    path: '/rules',
    title: 'Custom Rules',
    section: 'filtering',
    // One entry read and one user-triggered write. No event would change what
    // this page renders, and there is nothing here to poll.
    events: [],
    endpoints: [],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/rules'),
  },
  {
    path: '/policies',
    title: 'Policies',
    section: 'filtering',
    // Three entry one-shots. `active_assignments` moves on its own as a
    // schedule boundary passes, but a standing timer for one figure is exactly
    // what the route-scoped invariant rules out — it is re-read on entry.
    events: [],
    endpoints: [],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/policies'),
  },
  {
    path: '/clients',
    title: 'Clients',
    section: 'filtering',
    // Two entry one-shots and three user-triggered writes. `clients` is a
    // `REFRESH_ENDPOINTS` member, but subscribing to it would start a timer,
    // and the page whose job is current state must not be reading it through a
    // five-minute preference either — so this route declares neither.
    events: [],
    endpoints: [],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/clients'),
  },
  {
    path: '/rule-tester',
    title: 'Rule Tester',
    section: 'filtering',
    // Two entry one-shots and one user-triggered POST. The test reads the
    // running matcher; there is nothing about it to poll or subscribe to.
    events: [],
    endpoints: [],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/rule-tester'),
  },
  {
    path: '/cache',
    title: 'Cache',
    section: 'runtime',
    events: [],
    // `telemetry` joined in p5-08: the SWR and background-cleanup panels are
    // `counters.swr` and `counters.cache_cleanup`, which live there and nowhere
    // else. Read through the same shared refresh as `/cache`, so the page still
    // holds no timer of its own.
    endpoints: ['cache', 'telemetry'],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/cache'),
  },
  {
    // `/history/perf` is a range query, not a poll — no endpoint to declare.
    path: '/performance',
    title: 'Performance',
    section: 'runtime',
    events: [],
    endpoints: [],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/performance'),
  },
  {
    path: '/upstreams',
    title: 'Upstreams',
    section: 'runtime',
    events: [],
    // Plus one `GET /config` on mount and never again: the strategy is
    // boot-only, so it cannot change under a running process and a re-read
    // would answer the same thing for ever.
    endpoints: ['telemetry', 'health'],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/upstreams'),
  },
  {
    path: '/settings',
    title: 'Settings',
    section: 'system',
    // `config_changed` and nothing else. The page reads `/config` on entry and
    // on that event; `/health` is read once on entry and only while the restart
    // banner is armed, which is a revalidation rather than a poll — so no
    // endpoint is declared and this route holds no timer.
    events: ['config_changed'],
    endpoints: [],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/settings'),
  },
  {
    path: '/diagnostics/health',
    title: 'Health',
    section: 'system',
    group: 'diagnostics',
    events: [],
    // `lists` joined in p5-09: the list-problem summary is `GET /lists`' items
    // and lives nowhere else. Plus one `GET /config` on mount for the upstream
    // strategy — boot-only, so a re-read would answer the same thing for ever.
    endpoints: ['health', 'telemetry', 'lists'],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/diagnostics-health'),
  },
  {
    // Three reads, none of them polled: `/debug/memory` on entry for the
    // instant, `/telemetry` on entry for the compiled rule count and uptime
    // that `/debug/memory` does not carry, and `/history/perf` as a range
    // query. `endpoints` declares polled slots, so it stays empty — the page
    // holds no timer.
    path: '/diagnostics/memory',
    title: 'Memory',
    section: 'system',
    group: 'diagnostics',
    events: [],
    endpoints: [],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/diagnostics-memory'),
  },
  {
    path: '/diagnostics/live-feed',
    title: 'Live Feed',
    section: 'system',
    group: 'diagnostics',
    // The only screen that subscribes to `query`, and it declares no polled
    // endpoint: the feed's whole source is the socket. Leaving empties the
    // union, which closes the connection — and the engine stops publishing per
    // query once no socket asks for it at all.
    events: ['query'],
    endpoints: [],
    built: true,
    ownsHeader: true,
    load: () => import('../pages/live-feed'),
  },
];

export const LOGIN_ROUTE: Route = {
  path: '/login',
  title: 'Sign in',
  section: 'overview',
  events: [],
  endpoints: [],
  built: true,
  load: () => import('../pages/login'),
};

/** Registered only under `import.meta.env.DEV`; its absence from a production
 *  build is asserted by the postbuild grep, not trusted to tree-shaking. */
export const GALLERY_ROUTE: Route = {
  path: '/dev/gallery',
  title: 'Component gallery',
  section: 'system',
  events: ['stats'],
  endpoints: ['telemetry', 'cache'],
  built: true,
  // The guard is what keeps the chunk out of a production build: Vite replaces
  // `import.meta.env.DEV` with `false`, and the dead branch takes the dynamic
  // import with it. An unguarded `import()` here would still be emitted, since
  // reachability is decided statically.
  load: import.meta.env.DEV ? () => import('../pages/dev-gallery') : null,
};

export function navigableRoutes(): readonly Route[] {
  return import.meta.env.DEV ? [...ROUTES, GALLERY_ROUTE] : ROUTES;
}

export function allRoutes(): readonly Route[] {
  return [...navigableRoutes(), LOGIN_ROUTE];
}

export function routeFor(path: string): Route | null {
  return allRoutes().find((route) => route.path === path) ?? null;
}

/**
 * What the shell actually acquires. An unbuilt route renders nothing, so it
 * declares nothing — D7, and what keeps the indicator honestly reading
 * `not needed here` on every shipped route.
 */
export function effectiveEvents(route: Route): readonly EventType[] {
  return route.built ? route.events : [];
}

export function effectiveEndpoints(route: Route): readonly RefreshEndpoint[] {
  return route.built ? route.endpoints : [];
}

export const SECTION_LABELS: Record<Section, string> = {
  overview: 'Overview',
  filtering: 'Filtering',
  runtime: 'Runtime',
  system: 'System',
};
