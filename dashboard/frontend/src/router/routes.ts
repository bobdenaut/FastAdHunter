import type { ComponentType } from 'preact';
import { EVENT_TYPES, type EventType } from '../events/types';

/** The three endpoints the shared bounded refresh reads. Nothing pushed is
 *  polled: `stats` arrives on the socket and is never in this set. */
export const REFRESH_ENDPOINTS = ['health', 'telemetry', 'cache'] as const;
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
    events: ['stats'],
    endpoints: ['telemetry', 'cache'],
    built: false,
    load: null,
  },
  {
    path: '/lists',
    title: 'Lists',
    section: 'filtering',
    events: ['list_refreshed'],
    endpoints: [],
    built: false,
    load: null,
  },
  {
    path: '/rules',
    title: 'Custom Rules',
    section: 'filtering',
    events: [],
    endpoints: [],
    built: false,
    load: null,
  },
  {
    path: '/policies',
    title: 'Policies',
    section: 'filtering',
    events: [],
    endpoints: [],
    built: false,
    load: null,
  },
  {
    path: '/clients',
    title: 'Clients',
    section: 'filtering',
    events: [],
    endpoints: [],
    built: false,
    load: null,
  },
  {
    path: '/rule-tester',
    title: 'Rule Tester',
    section: 'filtering',
    events: [],
    endpoints: [],
    built: false,
    load: null,
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
