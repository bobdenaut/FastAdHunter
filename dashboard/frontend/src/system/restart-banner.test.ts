import { beforeEach, describe, expect, it } from 'vitest';
import {
  armRestartBanner,
  clearIfRestarted,
  observeHealth,
  resetRestartBanner,
  restartArming,
  subscribeRestartBanner,
  type RestartArming,
} from './restart-banner';

/**
 * The clear condition is the whole of this module's correctness: it must clear
 * on a restart and must not clear on a `/health` reading that predates one.
 *
 * Both times are the **client's own clock** and `uptime_seconds` is a duration
 * the server measured, so `nowMs − uptime · 1000 > armedAtMs` compares two
 * points on one clock and needs no skew tolerance. A comparison against a
 * server timestamp would.
 */

const MINUTE = 60_000;
const T0 = 1_700_000_000_000;

beforeEach(() => {
  resetRestartBanner();
});

describe('arming', () => {
  it('starts clear', () => {
    expect(restartArming()).toBeNull();
  });

  it('carries the keys this browser submitted', () => {
    armRestartBanner(['dns.cache.max_entries'], T0);
    expect(restartArming()).toEqual<RestartArming>({
      armedAtMs: T0,
      keys: ['dns.cache.max_entries'],
    });
  });

  it('carries no keys when the arming came from the event', () => {
    // `config_changed` carries `restart_required` and nothing else, so an
    // externally-armed banner names no key it never saw.
    armRestartBanner([], T0);
    expect(restartArming()?.keys).toEqual([]);
  });

  it('unions the keys and takes the later timestamp on a re-arm', () => {
    armRestartBanner(['dns.cache.max_entries'], T0);
    armRestartBanner(['dns.cache.max_entries', 'log.level'], T0 + MINUTE);
    expect(restartArming()).toEqual<RestartArming>({
      armedAtMs: T0 + MINUTE,
      keys: ['dns.cache.max_entries', 'log.level'],
    });
  });

  it('announces to its subscribers and stops on release', () => {
    const seen: Array<RestartArming | null> = [];
    const release = subscribeRestartBanner((state) => seen.push(state));
    armRestartBanner(['log.level'], T0);
    release();
    armRestartBanner(['log.format'], T0 + 1);
    expect(seen).toHaveLength(1);
    expect(seen[0]?.keys).toEqual(['log.level']);
  });
});

describe('clearing on an observed restart', () => {
  it('clears when the process booted after the change was saved', () => {
    armRestartBanner(['dns.cache.max_entries'], T0);
    // Read a minute later, 10 s of uptime: this boot started at T0 + 50 s.
    expect(clearIfRestarted({ uptime_seconds: 10 }, T0 + MINUTE)).toBe(true);
    expect(restartArming()).toBeNull();
  });

  it('does not clear while the process predates the change', () => {
    armRestartBanner(['dns.cache.max_entries'], T0);
    // Four hours of uptime: this is the process the change was saved against.
    expect(clearIfRestarted({ uptime_seconds: 4 * 3600 }, T0 + MINUTE)).toBe(
      false,
    );
    expect(restartArming()).not.toBeNull();
  });

  it('treats a boot exactly at the arming instant as not a restart', () => {
    // The change was saved by *this* process; `>` rather than `>=` is what
    // keeps a same-instant reading from clearing a real pending change.
    armRestartBanner(['log.level'], T0);
    expect(clearIfRestarted({ uptime_seconds: 60 }, T0 + MINUTE)).toBe(false);
  });

  it('does nothing while nothing is armed', () => {
    expect(clearIfRestarted({ uptime_seconds: 1 }, T0)).toBe(false);
  });

  it('ignores a reading with no usable uptime rather than clearing', () => {
    armRestartBanner(['log.level'], T0);
    for (const body of [null, {}, { uptime_seconds: 'soon' }, 42]) {
      expect(clearIfRestarted(body, T0 + MINUTE)).toBe(false);
    }
    expect(restartArming()).not.toBeNull();
  });
});

describe('the shared-refresh tap', () => {
  it('clears from an announcement another page caused', () => {
    armRestartBanner(['dns.cache.max_entries'], T0);
    // Read a minute after arming, five seconds of uptime: the boot is at
    // T0 + 55 s, which is after the arming — a real restart.
    observeHealth({
      data: { uptime_seconds: 5 },
      error: null,
      fetchedAt: T0 + MINUTE,
      pending: false,
    });
    expect(restartArming()).toBeNull();
  });

  it('ignores a pending announcement, which carries the previous reading', () => {
    armRestartBanner(['dns.cache.max_entries'], T0);
    observeHealth({
      data: null,
      error: null,
      fetchedAt: null,
      pending: true,
    });
    expect(restartArming()).not.toBeNull();
  });

  /**
   * The regression: `announce` fires once at fetch start with `pending: true`
   * and the **retained** payload. Judging that against the current clock —
   * rather than against the instant the reading was taken — places the boot as
   * late as the reading is stale, and clears the banner over a restart that
   * never happened. Both halves are asserted: the pending announcement is
   * skipped, and the settled one is judged at its own `fetchedAt`.
   */
  it('does not clear from a stale reading that predates the arming', () => {
    armRestartBanner(['dns.cache.max_entries'], T0);
    // Taken at the arming instant, uptime 5 s: this process booted at T0 − 5 s,
    // *before* the change was saved. A minute later the reading still says so.
    const stale = {
      data: { uptime_seconds: 5 },
      error: null,
      fetchedAt: T0,
    };
    observeHealth({ ...stale, pending: true });
    expect(restartArming()).not.toBeNull();
    observeHealth({ ...stale, pending: false });
    expect(restartArming()).not.toBeNull();
  });

  it('ignores a settled reading that never carried a fetch time', () => {
    armRestartBanner(['dns.cache.max_entries'], T0);
    observeHealth({
      data: { uptime_seconds: 5 },
      error: null,
      fetchedAt: null,
      pending: false,
    });
    expect(restartArming()).not.toBeNull();
  });

  it('ignores a failed read, which says nothing about a restart', () => {
    armRestartBanner(['dns.cache.max_entries'], T0);
    observeHealth({
      data: { uptime_seconds: 5 },
      error: new Error('unreachable'),
      fetchedAt: T0 + MINUTE,
      pending: false,
    });
    expect(restartArming()).not.toBeNull();
  });
});
