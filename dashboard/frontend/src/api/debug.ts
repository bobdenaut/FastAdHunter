import { request } from './core';
import type { DebugMemory } from './types';

export const DEBUG_MEMORY_PATH = '/api/v1/debug/memory';

/**
 * One instant of the memory breakdown, read once on entering the Memory page
 * and on nothing else — the trend comes from `/history/perf`, which is a range
 * query. This endpoint is not a `REFRESH_ENDPOINTS` member and starts no timer.
 *
 * It is `/telemetry`'s `memory` block plus the two allocator counters, gathered
 * in the same pass, so the two endpoints can never report a different RSS or
 * residual for one instant (API.md §Debug).
 */
export function getDebugMemory(signal?: AbortSignal): Promise<DebugMemory> {
  return request<DebugMemory>(DEBUG_MEMORY_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}
