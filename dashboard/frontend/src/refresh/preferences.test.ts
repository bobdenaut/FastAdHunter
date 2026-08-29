import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { REFRESH_DEFAULT_SECS, REFRESH_OPTIONS_SECS } from '../constants';
import { REFRESH_ENDPOINTS } from '../router/routes';
import {
  intervalOptions,
  isOfferedInterval,
  onStorageEvent,
  preferredIntervalSecs,
  subscribePreferences,
  writePreferredInterval,
} from './preferences';

const store = new Map<string, string>();
let throwing = false;

beforeEach(() => {
  store.clear();
  throwing = false;
  vi.stubGlobal('localStorage', {
    getItem(key: string) {
      if (throwing) throw new Error('site data blocked');
      return store.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      if (throwing) throw new Error('site data blocked');
      store.set(key, value);
    },
  });
  vi.stubGlobal('window', {
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the option sets are the validator', () => {
  it('offers exactly what the plan settled', () => {
    expect(intervalOptions('health')).toEqual([30, 60, 300]);
    expect(intervalOptions('telemetry')).toEqual([60, 300]);
    expect(intervalOptions('cache')).toEqual([60, 300]);
    expect(intervalOptions('clients')).toEqual([60, 300]);
    expect(intervalOptions('lists')).toEqual([60, 300]);
  });

  it('refuses a value the UI cannot offer', () => {
    expect(isOfferedInterval('telemetry', 30)).toBe(false);
    expect(isOfferedInterval('health', 30)).toBe(true);
    expect(writePreferredInterval('telemetry', 30)).toBe(false);
    expect(store.size).toBe(0);
  });

  it('falls back to the compiled default for a hand-edited value', () => {
    store.set('fah-refresh-health', '1');
    expect(preferredIntervalSecs('health')).toBe(REFRESH_DEFAULT_SECS.health);
    store.set('fah-refresh-health', 'soon');
    expect(preferredIntervalSecs('health')).toBe(REFRESH_DEFAULT_SECS.health);
  });

  it('has a default that is itself an offered option', () => {
    for (const endpoint of REFRESH_ENDPOINTS) {
      expect(REFRESH_OPTIONS_SECS[endpoint] as readonly number[]).toContain(
        REFRESH_DEFAULT_SECS[endpoint],
      );
    }
  });

  it('gives every polled endpoint a key, an option set and a default', () => {
    for (const endpoint of REFRESH_ENDPOINTS) {
      expect(intervalOptions(endpoint).length).toBeGreaterThan(0);
      expect(preferredIntervalSecs(endpoint)).toBe(
        REFRESH_DEFAULT_SECS[endpoint],
      );
    }
  });
});

describe('the two endpoints p5-06 added', () => {
  it('round-trips a stored interval', () => {
    for (const endpoint of ['clients', 'lists'] as const) {
      expect(writePreferredInterval(endpoint, 60)).toBe(true);
      expect(preferredIntervalSecs(endpoint)).toBe(60);
    }
    expect(store.get('fah-refresh-clients')).toBe('60');
    expect(store.get('fah-refresh-lists')).toBe('60');
  });

  it('falls back to the default for a hand-edited value', () => {
    store.set('fah-refresh-lists', '5');
    expect(preferredIntervalSecs('lists')).toBe(REFRESH_DEFAULT_SECS.lists);
    expect(writePreferredInterval('clients', 5)).toBe(false);
  });

  it('propagates a change from another tab, like the original three', () => {
    const seen: Array<[string, number]> = [];
    const release = subscribePreferences((endpoint, seconds) =>
      seen.push([endpoint, seconds]),
    );
    store.set('fah-refresh-clients', '60');
    onStorageEvent({ key: 'fah-refresh-clients' });
    store.set('fah-refresh-lists', '60');
    onStorageEvent({ key: 'fah-refresh-lists' });
    release();
    expect(seen).toEqual([
      ['clients', 60],
      ['lists', 60],
    ]);
  });
});

describe('storage that throws', () => {
  it('falls back on read and does not crash on write', () => {
    throwing = true;
    expect(preferredIntervalSecs('cache')).toBe(REFRESH_DEFAULT_SECS.cache);
    expect(() => writePreferredInterval('cache', 60)).not.toThrow();
    expect(writePreferredInterval('cache', 60)).toBe(true);
  });
});

describe('sharing', () => {
  it('tells every subscriber about a change', () => {
    const seen: Array<[string, number]> = [];
    const release = subscribePreferences((endpoint, seconds) =>
      seen.push([endpoint, seconds]),
    );
    writePreferredInterval('telemetry', 60);
    release();
    writePreferredInterval('telemetry', 300);
    expect(seen).toEqual([['telemetry', 60]]);
  });

  it('installs the cross-tab listener only while something is subscribed', () => {
    const release = subscribePreferences(() => {});
    expect(window.addEventListener).toHaveBeenCalledWith(
      'storage',
      expect.any(Function),
    );
    release();
    expect(window.removeEventListener).toHaveBeenCalledWith(
      'storage',
      expect.any(Function),
    );
  });

  it('applies another tab’s change exactly as if it were local', () => {
    const seen: Array<[string, number]> = [];
    const release = subscribePreferences((endpoint, seconds) =>
      seen.push([endpoint, seconds]),
    );
    store.set('fah-refresh-telemetry', '60');
    onStorageEvent({ key: 'fah-refresh-telemetry' });
    release();
    expect(seen).toEqual([['telemetry', 60]]);
  });

  it('ignores a storage event for an unrelated key', () => {
    const listener = vi.fn();
    const release = subscribePreferences(listener);
    onStorageEvent({ key: 'fah-theme' });
    onStorageEvent({ key: null });
    release();
    expect(listener).not.toHaveBeenCalled();
  });
});
