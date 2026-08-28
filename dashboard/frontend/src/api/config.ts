import { request } from './core';
import type { ApiKeyResponse, Config, ConfigUpdateResponse } from './types';

export const CONFIG_PATH = '/api/v1/config';
export const APIKEY_ROTATE_PATH = '/api/v1/config/apikey/rotate';

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

/**
 * A **partial** deep-merge: the body carries the changed keys and nothing else.
 * Submitting the document that was read back would write every value the UI
 * last saw, keys it does not model included — so a hand-edited value, or one a
 * newer build added, would be overwritten the moment anyone saved an unrelated
 * field (IA §Writing config).
 *
 * No `AbortSignal`: a write must land. There is nothing route-scoped for a late
 * response to mutate.
 */
export function postConfig(patch: unknown): Promise<ConfigUpdateResponse> {
  return request<ConfigUpdateResponse>(CONFIG_PATH, {
    method: 'POST',
    body: patch,
  });
}

/** The new key is returned **once** and the old one stops working immediately.
 *  It lives beside its route family rather than in `auth.ts`. */
export function rotateApiKey(): Promise<ApiKeyResponse> {
  return request<ApiKeyResponse>(APIKEY_ROTATE_PATH, { method: 'POST' });
}
