import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AGE_TICK_MS } from '../constants';
import { after, ageTickerRunning, every, subscribeAgeTick } from './timers';

const SCHEDULERS = ['setInterval', 'setTimeout', 'requestAnimationFrame'];

function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) sourceFiles(full, out);
    else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

describe('timer ownership', () => {
  it('is the only module that schedules anything', () => {
    const root = join(import.meta.dirname, '..');
    const offenders: string[] = [];
    for (const file of sourceFiles(root)) {
      if (file.endsWith(join('lifecycle', 'timers.ts'))) continue;
      const source = readFileSync(file, 'utf8');
      for (const token of SCHEDULERS) {
        // Method names included: this is why the registry's selector API is
        // called `setRefreshInterval` — a grep cannot tell a method from the
        // global.
        if (source.includes(token)) offenders.push(`${file}: ${token}`);
      }
    }
    expect(offenders).toEqual([]);
  });
});

describe('the shared age ticker', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('runs one timer for any number of subscribers', () => {
    const first = vi.fn();
    const second = vi.fn();
    const releaseFirst = subscribeAgeTick(first);
    const releaseSecond = subscribeAgeTick(second);
    expect(ageTickerRunning()).toBe(true);
    expect(vi.getTimerCount()).toBe(1);

    vi.advanceTimersByTime(AGE_TICK_MS);
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);

    releaseFirst();
    releaseSecond();
  });

  it('is alive only while something is mounted', () => {
    expect(ageTickerRunning()).toBe(false);
    const release = subscribeAgeTick(() => {});
    expect(ageTickerRunning()).toBe(true);
    release();
    expect(ageTickerRunning()).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('stops delivering to a released subscriber', () => {
    const listener = vi.fn();
    const release = subscribeAgeTick(listener);
    release();
    const keepAlive = subscribeAgeTick(() => {});
    vi.advanceTimersByTime(AGE_TICK_MS * 3);
    expect(listener).not.toHaveBeenCalled();
    keepAlive();
  });
});

describe('the primitives', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('cancel is idempotent for both', () => {
    const oneShot = vi.fn();
    const cancelOnce = after(50, oneShot);
    cancelOnce();
    cancelOnce();
    vi.advanceTimersByTime(100);
    expect(oneShot).not.toHaveBeenCalled();

    const repeating = vi.fn();
    const cancelEvery = every(50, repeating);
    vi.advanceTimersByTime(120);
    cancelEvery();
    cancelEvery();
    vi.advanceTimersByTime(500);
    expect(repeating).toHaveBeenCalledTimes(2);
  });
});
