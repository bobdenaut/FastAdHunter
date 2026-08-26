import { describe, expect, it } from 'vitest';
import {
  BACKOFF_JITTER,
  BACKOFF_MS,
  OPEN_STABLE_MS,
  PROBE_FAILURE_WINDOW_MS,
} from '../constants';
import { backoffDelay, isImmediateFailure, shouldResetAttempts } from './backoff';

describe('backoff', () => {
  it('walks the schedule and caps at the last step', () => {
    const mid = () => 0.5;
    for (let attempt = 0; attempt < BACKOFF_MS.length; attempt += 1) {
      expect(backoffDelay(attempt, mid)).toBe(BACKOFF_MS[attempt]);
    }
    expect(backoffDelay(99, mid)).toBe(BACKOFF_MS[BACKOFF_MS.length - 1]);
  });

  it('keeps jitter inside ±20 %', () => {
    for (let attempt = 0; attempt < BACKOFF_MS.length; attempt += 1) {
      const base = BACKOFF_MS[attempt] as number;
      for (const random of [() => 0, () => 1, () => 0.5, () => 0.13]) {
        const delay = backoffDelay(attempt, random);
        expect(delay).toBeGreaterThanOrEqual(Math.round(base * (1 - BACKOFF_JITTER)));
        expect(delay).toBeLessThanOrEqual(Math.round(base * (1 + BACKOFF_JITTER)));
      }
    }
  });

  it('treats a negative attempt as the first one', () => {
    expect(backoffDelay(-1, () => 0.5)).toBe(BACKOFF_MS[0]);
  });
});

describe('the immediate-failure detector', () => {
  it('counts a close that never opened, inside the window', () => {
    expect(isImmediateFailure(false, 0)).toBe(true);
    expect(isImmediateFailure(false, PROBE_FAILURE_WINDOW_MS - 1)).toBe(true);
  });

  it('does not count a close after a healthy open', () => {
    expect(isImmediateFailure(true, 0)).toBe(false);
    expect(isImmediateFailure(true, 60_000)).toBe(false);
  });

  it('does not count a close outside the window', () => {
    expect(isImmediateFailure(false, PROBE_FAILURE_WINDOW_MS)).toBe(false);
  });
});

describe('the attempt-counter reset', () => {
  it('needs the socket to have held open for OPEN_STABLE_MS', () => {
    expect(shouldResetAttempts(OPEN_STABLE_MS)).toBe(true);
    expect(shouldResetAttempts(OPEN_STABLE_MS + 1)).toBe(true);
  });

  it('does not fire for an open that died sooner', () => {
    expect(shouldResetAttempts(OPEN_STABLE_MS - 1)).toBe(false);
    expect(shouldResetAttempts(0)).toBe(false);
  });
});
