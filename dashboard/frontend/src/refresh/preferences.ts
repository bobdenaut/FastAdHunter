import { REFRESH_DEFAULT_SECS, REFRESH_OPTIONS_SECS } from '../constants';
import type { RefreshEndpoint } from '../router/routes';

/**
 * One browser-local preference per endpoint. It is global per endpoint on
 * purpose: changing the `/telemetry` selector on any card changes it for every
 * card and every page reading `/telemetry`. This is one operator's viewing
 * preference for one browser — not per-widget state, and not a server setting
 * that would make one household member's taste global to every client.
 */

const KEYS: Record<RefreshEndpoint, string> = {
  health: 'fah-refresh-health',
  telemetry: 'fah-refresh-telemetry',
  cache: 'fah-refresh-cache',
  clients: 'fah-refresh-clients',
  lists: 'fah-refresh-lists',
};

type Listener = (endpoint: RefreshEndpoint, seconds: number) => void;

const listeners = new Set<Listener>();
let storageListening = false;

/** `REFRESH_OPTIONS_SECS` is both the dropdown's contents and the validator, so
 *  a stored value the UI cannot offer cannot survive a read. A hand-edited `1`
 *  must never become a 1 ms timer. */
export function isOfferedInterval(
  endpoint: RefreshEndpoint,
  seconds: number,
): boolean {
  return (REFRESH_OPTIONS_SECS[endpoint] as readonly number[]).includes(
    seconds,
  );
}

export function intervalOptions(endpoint: RefreshEndpoint): readonly number[] {
  return REFRESH_OPTIONS_SECS[endpoint];
}

export function preferredIntervalSecs(endpoint: RefreshEndpoint): number {
  const fallback = REFRESH_DEFAULT_SECS[endpoint];
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(KEYS[endpoint]);
  } catch {
    // A browser with site data blocked throws on read as well as on write.
    return fallback;
  }
  if (raw === null) return fallback;
  const seconds = Number(raw);
  return isOfferedInterval(endpoint, seconds) ? seconds : fallback;
}

/** Returns `false` for a value the UI does not offer; nothing is stored and no
 *  listener is told. */
export function writePreferredInterval(
  endpoint: RefreshEndpoint,
  seconds: number,
): boolean {
  if (!isOfferedInterval(endpoint, seconds)) return false;
  try {
    localStorage.setItem(KEYS[endpoint], String(seconds));
  } catch {
    // The choice still applies to this tab; it just will not survive a reload.
  }
  announce(endpoint, seconds);
  return true;
}

function announce(endpoint: RefreshEndpoint, seconds: number): void {
  for (const listener of listeners) listener(endpoint, seconds);
}

function endpointForKey(key: string | null): RefreshEndpoint | null {
  if (key === null) return null;
  for (const endpoint of Object.keys(KEYS) as RefreshEndpoint[]) {
    if (KEYS[endpoint] === key) return endpoint;
  }
  return null;
}

/**
 * A change made in another tab is applied exactly as if it were made locally.
 * The preference is described as browser-global; without this it would be
 * tab-global, which is a different and wronger thing.
 */
export function onStorageEvent(event: {
  key: string | null;
  newValue?: string | null;
}): void {
  const endpoint = endpointForKey(event.key);
  if (endpoint === null) return;
  announce(endpoint, preferredIntervalSecs(endpoint));
}

/**
 * Every mounted selector subscribes here and unsubscribes on unmount, so
 * changing one updates the others in the same render. Components never hold
 * their own copy.
 */
export function subscribePreferences(listener: Listener): () => void {
  listeners.add(listener);
  if (!storageListening && typeof window !== 'undefined') {
    window.addEventListener('storage', onStorageEvent);
    storageListening = true;
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && storageListening) {
      window.removeEventListener('storage', onStorageEvent);
      storageListening = false;
    }
  };
}
