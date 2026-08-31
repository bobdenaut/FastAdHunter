import { useCallback } from 'preact/hooks';
import {
  UPSTREAM_PERF_FIELDS,
  getHistoryPerf,
  type HistoryPerfQuery,
} from '../../api/history';
import type { Config, HistoryPerf } from '../../api/types';
import { useRecordedRange } from '../recorded';
import { RANGES, type RangeKey } from '../dashboard/ranges';

/**
 * The same three ranges the Dashboard and Performance draw, imported rather
 * than restated. Only the field list differs: this page wants the endpoints
 * and nothing else on the row, and `max_points` stays at the endpoint's own
 * default so a wide range decimates the way the other history charts do.
 */
export function rttQuery(range: RangeKey, now: number): HistoryPerfQuery {
  return {
    from: new Date(now - RANGES[range].spanMs).toISOString(),
    fields: UPSTREAM_PERF_FIELDS,
  };
}

export interface RttHistoryState {
  history: HistoryPerf | null;
  error: Error | null;
  loading: boolean;
  /** `false` only once `/config` has actually said so. */
  recording: boolean;
}

/**
 * `useRecordedRange` over `/history/perf`, third caller after the Dashboard's
 * summary chart and Performance — one request per range selection, one
 * `/config` disambiguation of an empty answer, the three failure paths stated
 * there and not restated here.
 */
export function useRttHistory(
  range: RangeKey,
  config: Config | null,
  setConfig: (config: Config) => void,
  readConfig: (signal: AbortSignal) => Promise<Config>,
): RttHistoryState {
  const read = useCallback(
    (now: number, signal: AbortSignal) =>
      getHistoryPerf(rttQuery(range, now), signal),
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
