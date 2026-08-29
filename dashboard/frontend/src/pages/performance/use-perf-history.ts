import { useCallback } from 'preact/hooks';
import { PERF_FIELDS, getHistoryPerf, type HistoryPerfQuery } from '../../api/history';
import type { Config, HistoryPerf } from '../../api/types';
import { useRecordedRange } from '../recorded';
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
 * `useRecordedRange` over `/history/perf` — the protocol (one request per
 * range selection, the single-flight `/config` disambiguation of an empty
 * answer, the three failure paths) lives in `pages/recorded.ts`, shared with
 * the Dashboard's summary chart. Only the query is this page's own.
 */
export function usePerfHistory(
  range: RangeKey,
  config: Config | null,
  setConfig: (config: Config) => void,
  readConfig: (signal: AbortSignal) => Promise<Config>,
): PerfHistoryState {
  const read = useCallback(
    (now: number, signal: AbortSignal) =>
      getHistoryPerf(perfQuery(range, now), signal),
    [range],
  );
  const state = useRecordedRange(read, config, setConfig, readConfig);
  return {
    history: state.data,
    error: state.error,
    loading: state.loading,
    recording: state.recording,
  };
}
