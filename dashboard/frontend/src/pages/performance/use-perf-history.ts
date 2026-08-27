import { useCallback, useEffect, useRef, useState } from 'preact/hooks';
import { PERF_FIELDS, getHistoryPerf, type HistoryPerfQuery } from '../../api/history';
import type { Config, HistoryPerf } from '../../api/types';
import { RANGES, type RangeKey } from '../dashboard/ranges';

/**
 * The three ranges are the Dashboard's, imported rather than restated — the
 * chips read the same and a fourth range should appear on both pages or on
 * neither. Only the query differs: `/history/perf` is a per-sample series and
 * takes no `resolution`, and `max_points` is left at the server's own perf
 * default of 1000, which is what makes `stride > 1` real at 7 d and 30 d.
 */
export function perfQuery(range: RangeKey, now: number): HistoryPerfQuery {
  return {
    from: new Date(now - RANGES[range].spanMs).toISOString(),
    fields: PERF_FIELDS,
  };
}

export interface PerfHistoryState {
  history: HistoryPerf | null;
  error: Error | null;
  loading: boolean;
  /** `false` only once `/config` has actually said so. */
  recording: boolean;
}

/**
 * One request per range selection, and the one place the two empty answers are
 * told apart.
 *
 * This whole page is persisted history, so with the recorder off it answers
 * `200` with empty `items` for ever and would read as "nothing happened".
 * `history.enabled` is runtime-mutable and this page holds no
 * `config_changed` subscription, so a mount snapshot can go stale in exactly
 * that one way — which is worth **one** extra `/config` read on an empty
 * response, triggered by that response and by nothing else.
 *
 * `readConfig` is the page's own reader rather than `getConfig` directly: an
 * empty first answer can beat the mount read home, and joining the in-flight
 * one is what keeps that from being two concurrent requests for the same
 * document.
 *
 * Failure paths, all three stated rather than improvised:
 *
 * - a failed `/history/perf` surfaces as an error the page renders and the
 *   chips stay live, so a retry is a range re-selection;
 * - a failed mount `/config` is a degraded rendering, not an error state — the
 *   recorder is assumed on and the charts draw normally;
 * - a failed disambiguation re-read settles loading into the per-card empty
 *   states rather than holding them past the round trip.
 */
export function usePerfHistory(
  range: RangeKey,
  config: Config | null,
  setConfig: (config: Config) => void,
  readConfig: (signal: AbortSignal) => Promise<Config>,
): PerfHistoryState {
  const [history, setHistory] = useState<HistoryPerf | null>(null);
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
    getHistoryPerf(perfQuery(range, Date.now()), controller.signal)
      .then((response) => {
        // The previous range's series stays plotted until this lands — only an
        // answer that would *be* an empty state waits.
        setHistory(response);
        setLoading(false);
        if (response.items.length === 0) disambiguate(controller.signal);
      })
      .catch((cause: unknown) => {
        if (cause instanceof Error && cause.name === 'AbortError') return;
        setError(cause instanceof Error ? cause : new Error(String(cause)));
        setLoading(false);
      });
    return () => controller.abort();
  }, [range, disambiguate]);

  return {
    history,
    error,
    // Held across the disambiguation round trip rather than flashing the wrong
    // empty state for one frame.
    loading: loading || recheck,
    recording: snapshotEnabled,
  };
}
