import { getCache } from '../api/cache';
import { getClients } from '../api/clients';
import { getHealth } from '../api/health';
import { getLists } from '../api/lists';
import { getTelemetry } from '../api/telemetry';
import { every, nowMs, type Cancel } from '../lifecycle/timers';
import { REFRESH_ENDPOINTS, type RefreshEndpoint } from '../router/routes';
import {
  preferredIntervalSecs,
  subscribePreferences,
  writePreferredInterval,
} from './preferences';

export interface EndpointState {
  data: unknown;
  error: Error | null;
  /** `null` until the first successful read. */
  fetchedAt: number | null;
  pending: boolean;
}

type Listener = (state: EndpointState) => void;
type Fetcher = (signal: AbortSignal) => Promise<unknown>;

const DEFAULT_FETCHERS: Record<RefreshEndpoint, Fetcher> = {
  health: (signal) => getHealth(signal),
  telemetry: (signal) => getTelemetry(signal),
  cache: (signal) => getCache(signal),
  clients: (signal) => getClients(signal),
  lists: (signal) => getLists(signal),
};

interface Slot {
  listeners: Set<Listener>;
  /** Survives the last unsubscribe; the timer does not. A retained value is a
   *  cache, never an implicit subscription. */
  data: unknown;
  error: Error | null;
  fetchedAt: number | null;
  inFlight: Promise<void> | null;
  controller: AbortController | null;
  timer: Cancel | null;
  /** A manual Refresh that joined a background request still owns the timer
   *  reset: the operator asked for "now", and a scheduled fetch arriving
   *  seconds later is the thing that reset exists to prevent. */
  restartTimerOnSuccess: boolean;
}

/**
 * Shared, not global. It polls an endpoint only while at least one mounted page
 * subscribes to it, and the last unsubscribe clears that timer. A refresher
 * that keeps reading `/telemetry` because it exists is the exact thing the
 * route-scoped invariant forbids.
 *
 * Nothing pushed is polled: `stats` arrives on the socket and is never here.
 */
export class RefreshRegistry {
  private readonly fetchers: Record<RefreshEndpoint, Fetcher>;
  private readonly slots = new Map<RefreshEndpoint, Slot>();
  /**
   * The passive taps. A separate map from `Slot.listeners`, and it is read in
   * exactly two places — `observe()` and `announce()`. Nothing in `subscribe`,
   * `startTimer`, `fetch`, `isStale` or `setSuspended` may look at it: the
   * moment one of them does, this is a second lifecycle mechanism rather than
   * a tap on the one that exists.
   */
  private readonly observers = new Map<RefreshEndpoint, Set<Listener>>();
  private readonly releasePreferences: () => void;
  private suspended = false;

  constructor(fetchers: Partial<Record<RefreshEndpoint, Fetcher>> = {}) {
    this.fetchers = { ...DEFAULT_FETCHERS, ...fetchers };
    // A change from another tab arrives here identically to a local one, so a
    // preference described as browser-global really is one.
    this.releasePreferences = subscribePreferences((endpoint) => {
      this.rebuildTimer(endpoint);
    });
  }

  dispose(): void {
    this.releasePreferences();
    for (const slot of this.slots.values()) this.stopTimer(slot);
  }

  subscribe(endpoint: RefreshEndpoint, listener: Listener): () => void {
    const slot = this.slotFor(endpoint);
    const first = slot.listeners.size === 0;
    slot.listeners.add(listener);

    // A later subscriber is served the retained value and causes no request.
    listener(stateOf(slot));

    if (first) {
      this.startTimer(endpoint, slot);
      if (!this.suspended) void this.fetch(endpoint, slot);
    }

    let released = false;
    return () => {
      if (released) return;
      released = true;
      slot.listeners.delete(listener);
      if (slot.listeners.size === 0) {
        this.stopTimer(slot);
        // Leaving a page stops the work that page was causing, in-flight
        // included. The request is shared, so this only fires once the last
        // subscriber is gone.
        slot.controller?.abort();
      }
    };
  }

  /**
   * A **passive tap**: the listener hears announcements an endpoint already
   * makes and causes none. It refcounts nothing, starts no timer and triggers
   * no fetch, so an endpoint with observers and no subscribers is read exactly
   * as often as one with neither — never.
   *
   * **There is no initial replay.** Unlike `subscribe`, this does not call back
   * with the retained state on registration, so it cannot be used as a data
   * source: a caller that needs the current value reads the endpoint itself.
   * That absence is what keeps this from growing into a second `useRefresh`.
   *
   * The restart banner is its one caller (`services.ts`): it needs to notice a
   * `/health` reading some other page's refresh happened to take, and a timer
   * of its own is exactly what a banner is not worth.
   */
  observe(endpoint: RefreshEndpoint, listener: Listener): () => void {
    const taps = this.observers.get(endpoint) ?? new Set<Listener>();
    this.observers.set(endpoint, taps);
    taps.add(listener);

    let released = false;
    return () => {
      if (released) return;
      released = true;
      taps.delete(listener);
    };
  }

  /** What the card's Refresh button calls. A click while a request is in flight
   *  joins it: never a second request, never a queue. */
  invalidate(endpoint: RefreshEndpoint): Promise<void> {
    const slot = this.slotFor(endpoint);
    return this.fetch(endpoint, slot, { restartTimer: true });
  }

  /**
   * The name carries `Refresh` deliberately: the scheduler token is banned
   * outside `lifecycle/timers.ts`, and the grep enforcing that cannot tell a
   * registry method from the global.
   *
   * Rebuilding restarts from now — elapsed time is discarded, not rebased.
   * Rebasing 300 s to 60 s after 200 s elapsed would fire instantly, and a
   * selector change must not issue a request.
   */
  setRefreshInterval(endpoint: RefreshEndpoint, seconds: number): boolean {
    return writePreferredInterval(endpoint, seconds);
  }

  private rebuildTimer(endpoint: RefreshEndpoint): void {
    const slot = this.slots.get(endpoint);
    if (slot === undefined || slot.listeners.size === 0) return;
    this.stopTimer(slot);
    this.startTimer(endpoint, slot);
  }

  /**
   * Set by the visibility signal, which owns no timer of its own. The registry
   * never observes `visibilitychange` itself.
   */
  setSuspended(suspended: boolean): void {
    if (this.suspended === suspended) return;
    this.suspended = suspended;
    if (suspended) {
      for (const slot of this.slots.values()) this.stopTimer(slot);
      return;
    }
    for (const endpoint of REFRESH_ENDPOINTS) {
      const slot = this.slots.get(endpoint);
      if (slot === undefined || slot.listeners.size === 0) continue;
      this.startTimer(endpoint, slot);
      if (this.isStale(endpoint, slot)) void this.fetch(endpoint, slot);
    }
  }

  /** Bounded by the endpoint set — at most five values plus their timestamps,
   *  never by uptime or by the number of pages visited. */
  retained(endpoint: RefreshEndpoint): EndpointState {
    return stateOf(this.slotFor(endpoint));
  }

  subscriberCount(endpoint: RefreshEndpoint): number {
    return this.slots.get(endpoint)?.listeners.size ?? 0;
  }

  activeTimers(): number {
    let count = 0;
    for (const slot of this.slots.values()) if (slot.timer !== null) count += 1;
    return count;
  }

  private slotFor(endpoint: RefreshEndpoint): Slot {
    let slot = this.slots.get(endpoint);
    if (slot === undefined) {
      slot = {
        listeners: new Set(),
        data: null,
        error: null,
        fetchedAt: null,
        inFlight: null,
        controller: null,
        timer: null,
        restartTimerOnSuccess: false,
      };
      this.slots.set(endpoint, slot);
    }
    return slot;
  }

  private isStale(endpoint: RefreshEndpoint, slot: Slot): boolean {
    if (slot.fetchedAt === null) return true;
    return nowMs() - slot.fetchedAt >= preferredIntervalSecs(endpoint) * 1000;
  }

  private startTimer(endpoint: RefreshEndpoint, slot: Slot): void {
    if (this.suspended || slot.timer !== null) return;
    const periodMs = preferredIntervalSecs(endpoint) * 1000;
    slot.timer = every(periodMs, () => {
      void this.fetch(endpoint, slot);
    });
  }

  private stopTimer(slot: Slot): void {
    if (slot.timer === null) return;
    slot.timer();
    slot.timer = null;
  }

  /** One in-flight promise per endpoint, shared by every caller: two cards on
   *  one route reading `/telemetry` cost one request, never two loops. */
  private fetch(
    endpoint: RefreshEndpoint,
    slot: Slot,
    options: { restartTimer?: boolean } = {},
  ): Promise<void> {
    if (options.restartTimer === true) slot.restartTimerOnSuccess = true;
    // A click while a request is in flight joins it: never a second request,
    // never a queue — but the joining click keeps its timer reset.
    if (slot.inFlight !== null) return slot.inFlight;

    const controller = new AbortController();
    slot.controller = controller;
    this.announce(endpoint, slot, true);

    const run = this.fetchers[endpoint](controller.signal)
      .then((data) => {
        slot.data = data;
        slot.error = null;
        slot.fetchedAt = nowMs();
        if (slot.restartTimerOnSuccess && slot.listeners.size > 0) {
          // A manual refresh must not be followed by a scheduled one seconds
          // later.
          this.stopTimer(slot);
          this.startTimer(endpoint, slot);
        }
      })
      .catch((error: unknown) => {
        // A failed refresh never blanks a card: the previous value stays and
        // the timer is untouched. There is no `401` case here — `api/core.ts`
        // owns that, so session semantics live in one place.
        if (error instanceof Error && error.name === 'AbortError') return;
        slot.error = error instanceof Error ? error : new Error(String(error));
      })
      .finally(() => {
        slot.inFlight = null;
        slot.controller = null;
        // Cleared on failure too: the timer is untouched by a failed refresh,
        // and the flag must not arm the next background fetch.
        slot.restartTimerOnSuccess = false;
        this.announce(endpoint, slot, false);
      });

    slot.inFlight = run;
    return run;
  }

  private announce(
    endpoint: RefreshEndpoint,
    slot: Slot,
    pending: boolean,
  ): void {
    const state = { ...stateOf(slot), pending };
    for (const listener of slot.listeners) listener(state);
    // Second, and last, of the two sites that read `observers`. A tap never
    // reaches a request: it only hears one that a subscriber already caused.
    for (const tap of this.observers.get(endpoint) ?? []) tap(state);
  }
}

function stateOf(slot: Slot): EndpointState {
  return {
    data: slot.data,
    error: slot.error,
    fetchedAt: slot.fetchedAt,
    pending: slot.inFlight !== null,
  };
}
