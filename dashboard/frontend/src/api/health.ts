import { request } from './core';
import type { Health } from './types';

export const HEALTH_PATH = '/health';

/** Unauthenticated, so the login page can show the version too. */
export function getHealth(signal?: AbortSignal): Promise<Health> {
  return request<Health>(HEALTH_PATH, {
    notifyUnauthorized: false,
    ...(signal === undefined ? {} : { signal }),
  });
}
