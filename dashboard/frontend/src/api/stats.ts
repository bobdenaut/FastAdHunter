import { request } from './core';
import type { Stats } from './types';

export const STATS_PATH = '/api/v1/stats';

/**
 * Called once on mount and never polled: the `stats` frame on `WS /events` is
 * byte-for-byte this payload and arrives every ~2 s. This exists only because
 * the first push is up to ~2 s away.
 */
export function getStats(signal?: AbortSignal): Promise<Stats> {
  return request<Stats>(STATS_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}
