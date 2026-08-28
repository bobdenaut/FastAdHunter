import { useCallback, useEffect, useRef, useState } from 'preact/hooks';
import { getConfig } from '../api/config';
import type { Config } from '../api/types';

/**
 * The two persisted-history pages' shared machinery: the Dashboard's summary
 * chart and the Performance page read different endpoints but run the same
 * protocol, and p5-06's F11 was the two of them drifting — one gained the
 * single-flight `/config` join and the other kept issuing a second concurrent
 * read. One implementation, so they cannot drift again.
 *
 * Settings is deliberately **not** a caller: its reader carries the
 * `startedAfter` ordering guard a `config_changed` subscriber needs, which
 * these mount-snapshot pages do not.
 */

/**
 * One `GET /config` in flight at a time: a caller arriving while one is
 * pending joins it. The mount snapshot and the disambiguation re-read are the
 * two callers, and an empty first range answer can land before the mount read
 * does — without the join that pair is two concurrent requests for the same
 * document.
 */
export function useConfigReader(): (signal: AbortSignal) => Promise<Config> {
  const inFlight = useRef<Promise<Config> | null>(null);
  return useCallback((signal: AbortSignal): Promise<Config> => {
    const pending = inFlight.current;
    if (pending !== null) return pending;
    const run = getConfig(signal);
    inFlight.current = run;
    const clear = () => {
      if (inFlight.current === run) inFlight.current = null;
    };
    run.then(clear, clear);
    return run;
  }, []);
}

export interface RecordedRange<T> {
  data: T | null;
  error: Error | null;
  loading: boolean;
  /** `false` only once `/config` has actually said so. */
  recording: boolean;
}

/**
 * One request per range selection, and the one place the two empty answers are
 * told apart.
 *
 * A history endpoint answers `200` with empty `items` for ever while the
 * recorder is off, and `history.enabled` is runtime-mutable while neither
 * caller holds a `config_changed` subscription — so a mount snapshot can go
 * stale in exactly that one way, which is worth **one** extra `/config` read
 * on an empty response, triggered by that response and by nothing else.
 *
 * Failure paths, all three stated rather than improvised:
 *
 * - a failed range read surfaces as an error the page renders and the chips
 *   stay live, so a retry is a range re-selection;
 * - a failed mount `/config` is a degraded rendering, not an error state — the
 *   recorder is assumed on and the charts draw normally;
 * - a failed disambiguation re-read settles loading into the empty states
 *   rather than holding them past the round trip.
 *
 * `read` closes over the caller's range and must be a `useCallback` keyed on
 * it — its identity is what re-issues the request.
 */
export function useRecordedRange<T extends { items: readonly unknown[] }>(
  read: (now: number, signal: AbortSignal) => Promise<T>,
  config: Config | null,
  setConfig: (config: Config) => void,
  readConfig: (signal: AbortSignal) => Promise<Config>,
): RecordedRange<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<Error | null>(null);
  const [loading, setLoading] = useState(true);
  const [recheck, setRecheck] = useState(false);

  const snapshotEnabled = config?.history?.enabled ?? true;
  const enabledRef = useRef(snapshotEnabled);
  enabledRef.current = snapshotEnabled;

  const disambiguate = useCallback(
    (signal: AbortSignal) => {
      if (!enabledRef.current) return;
      setRecheck(true);
      readConfig(signal)
        .then((fresh) => {
          setConfig(fresh);
          setRecheck(false);
        })
        .catch(() => setRecheck(false));
    },
    [readConfig, setConfig],
  );

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    read(Date.now(), controller.signal)
      .then((response) => {
        // The previous range's series stays plotted until this lands — only an
        // answer that would *be* an empty state waits.
        setData(response);
        setLoading(false);
        if (response.items.length === 0) disambiguate(controller.signal);
      })
      .catch((cause: unknown) => {
        if (cause instanceof Error && cause.name === 'AbortError') return;
        setError(cause instanceof Error ? cause : new Error(String(cause)));
        setLoading(false);
      });
    return () => controller.abort();
  }, [read, disambiguate]);

  return {
    data,
    error,
    // Held across the disambiguation round trip rather than flashing the wrong
    // empty state for one frame.
    loading: loading || recheck,
    recording: snapshotEnabled,
  };
}
