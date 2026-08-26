import {
  BACKOFF_JITTER,
  BACKOFF_MS,
  OPEN_STABLE_MS,
  PROBE_FAILURE_WINDOW_MS,
} from '../constants';

/**
 * `BACKOFF_MS[min(attempt, last)]` ± `BACKOFF_JITTER`. The jitter is what keeps
 * several tabs from resynchronizing their reconnects on the box.
 */
export function backoffDelay(
  attempt: number,
  random: () => number = Math.random,
): number {
  const index = Math.min(Math.max(attempt, 0), BACKOFF_MS.length - 1);
  const base = BACKOFF_MS[index] ?? BACKOFF_MS[BACKOFF_MS.length - 1] ?? 0;
  const spread = base * BACKOFF_JITTER;
  return Math.round(base + (random() * 2 - 1) * spread);
}

/**
 * A browser `WebSocket` exposes no HTTP status for a rejected upgrade: a `401`
 * arrives as `onerror` then `close(1006)`, indistinguishable from a dropped
 * network. An *immediate* failure is one that closed without ever firing
 * `open`, inside the window.
 *
 * The threshold is deliberately tolerant. A socket failing just inside or just
 * outside the window is classified differently for the same server behaviour;
 * that costs one extra probe, or one probe delayed by a backoff step, and
 * nothing else — the probe is authoritative, this only decides when to ask.
 */
export function isImmediateFailure(
  everOpened: boolean,
  elapsedMs: number,
): boolean {
  return !everOpened && elapsedMs < PROBE_FAILURE_WINDOW_MS;
}

/**
 * The attempt counter returns to zero when the socket has been `open` for at
 * least `OPEN_STABLE_MS` — an elapsed-time threshold against one state, rather
 * than "survives the first step", which two implementations could read
 * differently.
 */
export function shouldResetAttempts(openDurationMs: number): boolean {
  return openDurationMs >= OPEN_STABLE_MS;
}
