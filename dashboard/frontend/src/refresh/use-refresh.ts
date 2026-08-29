import { useEffect, useState } from 'preact/hooks';
import type { RefreshEndpoint } from '../router/routes';
import type { EndpointState, RefreshRegistry } from './registry';
import { preferredIntervalSecs, subscribePreferences } from './preferences';

const EMPTY: EndpointState = {
  data: null,
  error: null,
  fetchedAt: null,
  pending: false,
};

/**
 * A widget reads an endpoint through here; it never starts a timer and never
 * issues a request of its own. Ten widgets wanting the same response cost one
 * request.
 */
export function useRefresh<T>(
  registry: RefreshRegistry,
  endpoint: RefreshEndpoint,
): { data: T | null; error: Error | null; fetchedAt: number | null; pending: boolean } {
  const [state, setState] = useState<EndpointState>(EMPTY);

  useEffect(() => registry.subscribe(endpoint, setState), [registry, endpoint]);

  return {
    data: state.data as T | null,
    error: state.error,
    fetchedAt: state.fetchedAt,
    pending: state.pending,
  };
}

/** Every mounted selector renders from shared preference state, never from a
 *  local copy — two clusters for one endpoint on different routes would
 *  otherwise drift apart. */
export function usePreferredInterval(endpoint: RefreshEndpoint): number {
  const [seconds, setSeconds] = useState(() => preferredIntervalSecs(endpoint));

  useEffect(() => {
    setSeconds(preferredIntervalSecs(endpoint));
    return subscribePreferences((changed, next) => {
      if (changed === endpoint) setSeconds(next);
    });
  }, [endpoint]);

  return seconds;
}
