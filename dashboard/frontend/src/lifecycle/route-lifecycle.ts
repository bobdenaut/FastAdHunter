import type { SocketManager } from '../events/socket';
import type { SubscriptionRegistry } from '../events/subscriptions';
import type { RefreshRegistry } from '../refresh/registry';
import {
  REFRESH_ENDPOINTS,
  effectiveEndpoints,
  effectiveEvents,
  type EventType,
  type RefreshEndpoint,
  type Route,
} from '../router/routes';

export interface RouteLifecycleOptions {
  subscriptions: SubscriptionRegistry;
  refresh: RefreshRegistry;
  socket: SocketManager;
  /** `import.meta.env.DEV` in the application; explicit here so the assertion
   *  is testable without a build flag. */
  assert?: boolean;
}

/**
 * The shell's route transition, and the only caller of acquire/release. A page
 * component never calls `acquire`, `release`, `subscribe` or `unsubscribe`, so
 * a page cannot leak a subscription by forgetting an unmount path.
 */
export class RouteLifecycle {
  private readonly options: RouteLifecycleOptions;
  private releases: Array<() => void> = [];
  /**
   * What this object itself is holding, counted as it acquires and releases.
   * Widgets subscribe to the same registry through `useRefresh`, so the
   * registry's global count answers a different question than "did the
   * transition release what it took" — and a refcount rather than a copy of the
   * declaration is what makes the answer worth asking for: a release that never
   * ran leaves its endpoint above zero here, and one that ran twice takes it
   * below.
   */
  private readonly held = new Map<RefreshEndpoint, number>();
  private route: Route | null = null;
  private suspended = false;

  constructor(options: RouteLifecycleOptions) {
    this.options = options;
  }

  current(): Route | null {
    return this.route;
  }

  /**
   * Acquire-then-release, in that order: navigating between two routes that
   * both want `stats` must not tear the socket down and rebuild it, and it is
   * the incoming acquisition landing first that keeps the refcount above zero
   * across the transition. Releasing first would empty the union for the length
   * of one statement, which is long enough — the socket manager closes on the
   * announcement, not on a later tick.
   *
   * The transition runs to completion before a visibility change is applied —
   * interleaving them is what produces the race.
   */
  enter(route: Route | null): void {
    const outgoing = this.releases;
    this.releases = [];
    this.route = route;

    if (route !== null) {
      const events = effectiveEvents(route);
      if (events.length > 0) {
        this.releases.push(this.options.subscriptions.acquire(events));
      }
      for (const endpoint of effectiveEndpoints(route)) {
        // The shell holds the endpoint for the route; widgets add their own
        // listeners through `useRefresh` and share the one request.
        const release = this.options.refresh.subscribe(endpoint, () => {});
        this.count(endpoint, 1);
        this.releases.push(() => {
          release();
          this.count(endpoint, -1);
        });
      }
    }

    for (const release of outgoing) release();

    if (this.options.assert === true) this.assertMatchesDeclaration();
  }

  setSuspended(suspended: boolean): void {
    if (this.suspended === suspended) return;
    this.suspended = suspended;
    this.options.refresh.setSuspended(suspended);
    this.options.socket.setSuspended(suspended);
  }

  /**
   * The net under the framework-lifecycle dependency the invariant otherwise
   * rests on: after every transition the socket's union must equal the incoming
   * route's declared events, and this transition must be holding exactly the
   * incoming route's declared endpoints — no more, because the outgoing route's
   * release ran, and no fewer, because the acquisition did.
   *
   * It asks about **this object's** holdings, not the registry's global
   * subscriber count. A widget on the outgoing page is still mounted when the
   * transition runs — the shell renders the previous page component until a
   * later effect swaps it — so a global count reports every ordinary
   * navigation between two built pages as a leak. Indicting the common case is
   * how a net gets deleted rather than fixed.
   */
  private assertMatchesDeclaration(): void {
    const route = this.route;
    const events: readonly EventType[] =
      route === null ? [] : effectiveEvents(route);
    const endpoints: readonly RefreshEndpoint[] =
      route === null ? [] : effectiveEndpoints(route);

    // The registry is shared; the subscription registry is not — the
    // transition is its only caller, so the global union is this object's.
    const union = this.options.subscriptions.union();
    const declared = new Set<EventType>(events);
    if (
      union.length !== declared.size ||
      union.some((type) => !declared.has(type))
    ) {
      throw new Error(
        `route lifecycle: union [${union.join(',')}] does not match declaration [${events.join(',')}]`,
      );
    }

    for (const endpoint of REFRESH_ENDPOINTS) {
      const held = this.held.get(endpoint) ?? 0;
      const wanted = endpoints.includes(endpoint) ? 1 : 0;
      if (held !== wanted) {
        throw new Error(
          `route lifecycle: ${endpoint} held=${String(held)} declared=${String(wanted)}`,
        );
      }
    }
  }

  private count(endpoint: RefreshEndpoint, delta: number): void {
    this.held.set(endpoint, (this.held.get(endpoint) ?? 0) + delta);
  }
}
