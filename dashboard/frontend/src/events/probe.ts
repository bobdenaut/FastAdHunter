import { ApiError, NetworkError, request } from '../api/core';

/**
 * One authenticated REST call: config-derived, bounded by the 16-policy
 * ceiling, no traffic-dependent work, no side effects, and the smallest
 * authenticated payload in the documented surface.
 */
export const PROBE_PATH = '/api/v1/policies';

/**
 * The probe answers one question: is the session still valid. A `2xx` does not
 * prove the upgrade path is healthy and is not read as if it did.
 */
export type ProbeOutcome =
  | 'session-expired'
  | 'session-valid'
  | 'unreachable'
  | 'inconclusive';

export function classifyProbe(result: unknown, failed: boolean): ProbeOutcome {
  if (!failed) return 'session-valid';
  if (result instanceof ApiError) {
    if (result.status === 401) return 'session-expired';
    // A 5xx says the server is reachable and nothing about the session.
    return 'inconclusive';
  }
  if (result instanceof NetworkError) return 'unreachable';
  return 'inconclusive';
}

/**
 * Issued once and never retried, with no backoff of its own: a second retry
 * loop inside the recovery path of the first is how a reconnect storm gets
 * built.
 */
export async function runProbe(signal?: AbortSignal): Promise<ProbeOutcome> {
  try {
    await request<unknown>(PROBE_PATH, {
      // The socket manager decides what a probe `401` means; the shell-level
      // guard must not fire first and navigate out from under it.
      notifyUnauthorized: false,
      ...(signal === undefined ? {} : { signal }),
    });
    return classifyProbe(null, false);
  } catch (error) {
    return classifyProbe(error, true);
  }
}

/** Only a real authentication failure sends the user to login. */
export function sendsToLogin(outcome: ProbeOutcome): boolean {
  return outcome === 'session-expired';
}
