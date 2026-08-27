import { request } from './core';
import type { Config } from './types';

export const CONFIG_PATH = '/api/v1/config';

/**
 * The effective configuration, secrets redacted and auth material omitted
 * entirely. Read as a mount snapshot: `history.enabled` is runtime-mutable and
 * `dns.upstreams.strategy` is boot-only, so only the first can go stale within
 * a session and only it gets a re-read.
 */
export function getConfig(signal?: AbortSignal): Promise<Config> {
  return request<Config>(CONFIG_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}
