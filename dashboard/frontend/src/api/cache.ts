import { request } from './core';
import type { CacheUsage } from './types';

export const CACHE_PATH = '/api/v1/cache';

export function getCache(signal?: AbortSignal): Promise<CacheUsage> {
  return request<CacheUsage>(CACHE_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}
