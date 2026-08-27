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

export interface Route {
  path: string;
  title: string;
  section: Section;
  group?: 'diagnostics';
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
    endpoints: ['cache'],
    built: false,
    load: null,
  },
  {
    // `/history/perf` is a range query, not a poll — no endpoint to declare.
    path: '/performance',
    title: 'Performance',
    section: 'runtime',
    events: [],
    endpoints: [],
    built: false,
    load: null,
  },
  {
    path: '/upstreams',
    title: 'Upstreams',
    section: 'runtime',
    events: [],
    endpoints: ['telemetry', 'health'],
    built: false,
    load: null,
  },
  {
    path: '/settings',
    title: 'Settings',
    section: 'system',
    events: ['config_changed'],
    endpoints: [],
    built: false,
    load: () => import('../pages/system'),
  },
  {
    path: '/diagnostics/health',
    title: 'Health',
    section: 'system',
    group: 'diagnostics',
    events: [],
    endpoints: ['health', 'telemetry'],
    built: false,
    load: () => import('../pages/system'),
  },
  {
    path: '/diagnostics/memory',
    title: 'Memory',
    section: 'system',
    group: 'diagnostics',
    events: [],
    endpoints: [],
    built: false,
    load: () => import('../pages/system'),
  },
  {
    path: '/diagnostics/live-feed',
    title: 'Live Feed',
    section: 'system',
    group: 'diagnostics',
    events: ['query'],
    endpoints: [],
    built: false,
    load: () => import('../pages/system'),
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
