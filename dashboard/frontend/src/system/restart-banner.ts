import type { EndpointState } from '../refresh/registry';

/**
 * The restart-required banner: **global UI state without a global poll.**
 *
 * A boot-only change is persisted and waiting, and the operator has to be told
 * so wherever they navigate next — but nothing here starts a timer on its
 * behalf. It is fed from two places, both of which are readings something else
 * already took:
 *
 * 1. Settings does one `GET /health` on entry while the banner is armed;
 * 2. `services.ts` taps `RefreshRegistry.observe('health', …)`, which hears a
 *    `/health` announcement whenever a mounted page's shared refresh takes one.
 *
 * The consequence is deliberate and is stated on the banner: parked on the
 * Cache page, it will not clear live. A banner is not worth a standing poll.
 *
 * **In-memory only.** A hard reload forgets that a change is pending, because
 * the API offers no "pending boot-only changes" read and inventing one is out
 * of scope. The next save re-arms it.
 */

export interface RestartArming {
  /** Client clock at the moment the change was persisted. */
  armedAtMs: number;
  /**
   * The dotted keys this browser submitted. **Empty when the arming came from
   * a `config_changed` event**, which carries `restart_required` and nothing
   * else — so an externally-armed banner says a change is pending without
   * naming keys it never saw.
   */
  keys: readonly string[];
}

type Listener = (arming: RestartArming | null) => void;

let arming: RestartArming | null = null;
const listeners = new Set<Listener>();

function announce(): void {
  for (const listener of listeners) listener(arming);
}

export function restartArming(): RestartArming | null {
  return arming;
}

export function subscribeRestartBanner(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Arms, or re-arms with a later timestamp and the union of the keys. Re-arming
 * matters: a second save while the first is still pending must not leave the
 * clear condition anchored to the older boot.
 */
export function armRestartBanner(keys: readonly string[], nowMs: number): void {
  const merged = arming === null ? keys : [...new Set([...arming.keys, ...keys])];
  arming = { armedAtMs: nowMs, keys: [...merged].sort() };
  announce();
}

/** Test seam and sign-out reset; not reachable from the UI. */
export function resetRestartBanner(): void {
  arming = null;
  announce();
}

interface UptimeReading {
  uptime_seconds: number;
}

function uptimeOf(data: unknown): number | null {
  if (typeof data !== 'object' || data === null) return null;
  const value = (data as Partial<UptimeReading>).uptime_seconds;
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

/**
 * Clears the banner once a `/health` reading shows the process booted **after**
 * the arming.
 *
 * `nowMs − uptime_seconds · 1000` is this boot's start on the client's own
 * clock, and `armedAtMs` was taken from the same clock — so the comparison is
 * skew-free by construction: `uptime_seconds` is a duration the server
 * measured, never a server timestamp the client would have to trust.
 *
 * Returns whether it cleared, which is what the tests read.
 */
export function clearIfRestarted(data: unknown, nowMs: number): boolean {
  if (arming === null) return false;
  const uptime = uptimeOf(data);
  if (uptime === null) return false;
  if (nowMs - uptime * 1000 <= arming.armedAtMs) return false;
  arming = null;
  announce();
  return true;
}

/**
 * What `services.ts` hands each `/health` announcement.
 *
 * **A pending announcement is skipped, and the clock is the reading's own.**
 * `RefreshRegistry.announce` fires twice per fetch — once at the start with
 * `pending: true` and the **retained** payload, once on settle — so the first
 * carries the previous reading. Judging that against the current clock places
 * this boot as late as the reading is stale, and a banner armed within that
 * staleness of a real boot then clears over a restart that never happened.
 * `EndpointState.fetchedAt` is when the uptime was actually read, which is the
 * only instant the subtraction is true at.
 */
export function observeHealth(state: EndpointState): void {
  if (state.pending || state.error !== null || state.data === null) return;
  if (state.fetchedAt === null) return;
  clearIfRestarted(state.data, state.fetchedAt);
}
