import { SocketManager, type SocketLike } from './events/socket';
import { SubscriptionRegistry } from './events/subscriptions';
import { RouteLifecycle } from './lifecycle/route-lifecycle';
import { RefreshRegistry } from './refresh/registry';
import { endSession } from './session/session';

/**
 * The application's four long-lived objects, created once. They are wired here
 * rather than inside a component so the route transition, the socket and the
 * refresh registry cannot be duplicated by a re-render.
 */
export const subscriptions = new SubscriptionRegistry();

export const refresh = new RefreshRegistry();

/**
 * The browser's `WebSocket` handlers are typed with a `this` and an `Event`
 * the manager has no use for. Narrowing them here keeps `SocketLike` — the
 * seam the unit tests drive — free of DOM types.
 */
function openWebSocket(url: string): SocketLike {
  const raw = new WebSocket(url);
  return {
    send: (data) => raw.send(data),
    close: (code) => raw.close(code),
    set onopen(handler: (() => void) | null) {
      raw.onopen = handler === null ? null : () => handler();
    },
    get onopen() {
      return null;
    },
    set onclose(handler: (() => void) | null) {
      raw.onclose = handler === null ? null : () => handler();
    },
    get onclose() {
      return null;
    },
    set onerror(handler: (() => void) | null) {
      raw.onerror = handler === null ? null : () => handler();
    },
    get onerror() {
      return null;
    },
    set onmessage(handler: ((event: { data: unknown }) => void) | null) {
      raw.onmessage =
        handler === null ? null : (event) => handler({ data: event.data });
    },
    get onmessage() {
      return null;
    },
  };
}

export const socket = new SocketManager({
  subscriptions,
  open: openWebSocket,
  onAuthFailure: endSession,
});

export const routeLifecycle = new RouteLifecycle({
  subscriptions,
  refresh,
  socket,
  // Costs the production bundle nothing, and is the net under the
  // framework-lifecycle dependency the invariant otherwise rests on.
  assert: import.meta.env.DEV,
});
