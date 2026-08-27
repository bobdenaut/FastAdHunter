import { request } from './core';
import type { CacheCleanResponse, CacheUsage } from './types';

export const CACHE_PATH = '/api/v1/cache';
export const CACHE_CLEAN_PATH = '/api/v1/cache/clean';

export function getCache(signal?: AbortSignal): Promise<CacheUsage> {
  return request<CacheUsage>(CACHE_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}

/**
 * Removes expired entries now instead of waiting for capacity eviction.
 *
 * **Stale-window entries are kept unless `stale` is true.** They are the
 * serve-stale insurance an upstream outage is survived on, so the query
 * parameter is emitted only when the operator asked for it — the server's own
 * default is the same choice (`wire.rs` `CacheCleanParams`), and sending
 * `?stale=false` would state a decision the caller did not make.
 *
 * `freed_bytes` counts the removed entries' own heap only: a clean never
 * shrinks the table slab, which is why RSS does not fall by that amount.
 */
export function cleanCache(
  stale: boolean,
  signal?: AbortSignal,
): Promise<CacheCleanResponse> {
  return request<CacheCleanResponse>(
    stale ? `${CACHE_CLEAN_PATH}?stale=true` : CACHE_CLEAN_PATH,
    { method: 'POST', ...(signal === undefined ? {} : { signal }) },
  );
}
