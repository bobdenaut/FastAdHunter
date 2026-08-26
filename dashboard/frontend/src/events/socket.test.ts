import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  BACKOFF_MS,
  HIDDEN_CLOSE_GRACE_MS,
  OPEN_STABLE_MS,
  PROBE_AFTER_FAILURES,
  PROBE_DIAGNOSTIC_CYCLES,
} from '../constants';
import {
  SocketManager,
  UNREACHABLE_DETAIL,
  UPGRADE_REFUSED_DETAIL,
  type SocketLike,
} from './socket';
import { SubscriptionRegistry } from './subscriptions';
import type { ProbeOutcome } from './probe';

class FakeSocket implements SocketLike {
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  readonly sent: string[] = [];
  closedWith: number | null = null;

  send(data: string): void {
    this.sent.push(data);
  }

  close(code?: number): void {
    this.closedWith = code ?? 1000;
  }
}

interface Harness {
  registry: SubscriptionRegistry;
  manager: SocketManager;
  sockets: FakeSocket[];
  authFailures: number;
  probes: number;
  setProbeOutcome: (outcome: ProbeOutcome) => void;
  drops: string[];
}

function harness(): Harness {
  const registry = new SubscriptionRegistry();
  const sockets: FakeSocket[] = [];
  const drops: string[] = [];
  let outcome: ProbeOutcome = 'inconclusive';
  const state = { authFailures: 0, probes: 0 };

  const manager = new SocketManager({
    subscriptions: registry,
    url: 'wss://box/api/v1/events',
    open: () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket;
    },
    onAuthFailure: () => {
      state.authFailures += 1;
    },
    probe: () => {
      state.probes += 1;
      return Promise.resolve(outcome);
    },
    random: () => 0.5,
    onDrop: (reason) => drops.push(reason),
  });

  return {
    registry,
    manager,
    sockets,
    drops,
    get authFailures() {
      return state.authFailures;
    },
    get probes() {
      return state.probes;
    },
    setProbeOutcome: (next: ProbeOutcome) => {
      outcome = next;
    },
  } as Harness;
}

function latest(sockets: FakeSocket[]): FakeSocket {
  const socket = sockets[sockets.length - 1];
  if (socket === undefined) throw new Error('no socket was opened');
  return socket;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('opening and closing', () => {
  it('stays closed until something acquires an event type', () => {
    const h = harness();
    expect(h.manager.state()).toBe('closed');
    expect(h.manager.indicator()).toBe('not-needed-here');
    expect(h.sockets).toHaveLength(0);
    h.manager.dispose();
  });

  it('opens on the first acquisition and sends the union', () => {
    const h = harness();
    h.registry.acquire(['stats']);
    expect(h.manager.state()).toBe('connecting');
    expect(h.manager.indicator()).toBe('reconnecting');

    latest(h.sockets).onopen?.();
    expect(h.manager.state()).toBe('open');
    expect(h.manager.indicator()).toBe('live');
    expect(latest(h.sockets).sent).toEqual(['{"subscribe":["stats"]}']);
    h.manager.dispose();
  });

  it('sends the new union when it changes while open', () => {
    const h = harness();
    h.registry.acquire(['stats']);
    latest(h.sockets).onopen?.();
    h.registry.acquire(['query']);
    expect(latest(h.sockets).sent).toEqual([
      '{"subscribe":["stats"]}',
      '{"subscribe":["query","stats"]}',
    ]);
    h.manager.dispose();
  });

  it('closes with 1000 when the union empties, and never sends an empty list', () => {
    const h = harness();
    const release = h.registry.acquire(['stats']);
    const socket = latest(h.sockets);
    socket.onopen?.();
    release();
    expect(socket.closedWith).toBe(1000);
    expect(h.manager.state()).toBe('closed');
    expect(h.manager.indicator()).toBe('not-needed-here');
    expect(socket.sent.some((frame) => frame.includes('[]'))).toBe(false);
    h.manager.dispose();
  });

  it('never opens for an empty union', () => {
    const h = harness();
    h.registry.acquire([])();
    expect(h.sockets).toHaveLength(0);
    h.manager.dispose();
  });

  it('does not tear the socket down between two routes that both want stats', () => {
    const h = harness();
    const leaving = h.registry.acquire(['stats']);
    const socket = latest(h.sockets);
    socket.onopen?.();

    const entering = h.registry.acquire(['stats']);
    leaving();

    expect(h.sockets).toHaveLength(1);
    expect(socket.closedWith).toBeNull();
    expect(socket.sent).toEqual(['{"subscribe":["stats"]}']);
    entering();
    h.manager.dispose();
  });
});

describe('reconnection', () => {
  it('backs off and re-sends the union after reconnecting', () => {
    const h = harness();
    h.registry.acquire(['stats', 'query']);
    const first = latest(h.sockets);
    first.onopen?.();
    vi.advanceTimersByTime(OPEN_STABLE_MS + 1);
    first.onclose?.();

    expect(h.manager.state()).toBe('backoff');
    expect(h.manager.indicator()).toBe('reconnecting');

    vi.advanceTimersByTime(BACKOFF_MS[0] as number);
    const second = latest(h.sockets);
    expect(second).not.toBe(first);
    second.onopen?.();
    expect(second.sent).toEqual(['{"subscribe":["query","stats"]}']);
    h.manager.dispose();
  });

  it('treats onerror as a close', () => {
    const h = harness();
    h.registry.acquire(['stats']);
    latest(h.sockets).onerror?.();
    expect(h.manager.state()).toBe('backoff');
    h.manager.dispose();
  });

  it('walks the backoff schedule while failures keep coming', () => {
    const h = harness();
    h.registry.acquire(['stats']);
    // Each failure is slow enough not to count as immediate, so no probe fires.
    for (let step = 0; step < 3; step += 1) {
      vi.advanceTimersByTime(5_000);
      latest(h.sockets).onclose?.();
      const opened = h.sockets.length;
      vi.advanceTimersByTime((BACKOFF_MS[step] as number) - 1);
      expect(h.sockets).toHaveLength(opened);
      vi.advanceTimersByTime(2);
      expect(h.sockets).toHaveLength(opened + 1);
    }
    h.manager.dispose();
  });
});

describe('the probe', () => {
  function failImmediately(h: Harness, times: number): void {
    for (let i = 0; i < times; i += 1) {
      latest(h.sockets).onclose?.();
      if (h.manager.state() === 'backoff') vi.advanceTimersToNextTimer();
    }
  }

  it('fires after PROBE_AFTER_FAILURES immediate failures', async () => {
    const h = harness();
    h.setProbeOutcome('inconclusive');
    h.registry.acquire(['stats']);
    failImmediately(h, PROBE_AFTER_FAILURES);
    expect(h.manager.state()).toBe('probing');
    await vi.advanceTimersByTimeAsync(0);
    expect(h.probes).toBe(1);
    h.manager.dispose();
  });

  it('sends the user to login only on an expired session', async () => {
    const h = harness();
    h.setProbeOutcome('session-expired');
    h.registry.acquire(['stats']);
    failImmediately(h, PROBE_AFTER_FAILURES);
    await vi.advanceTimersByTimeAsync(0);
    expect(h.authFailures).toBe(1);
    expect(h.manager.state()).toBe('closed');
    h.manager.dispose();
  });

  it('keeps backing off when the server is unreachable, and says so', async () => {
    const h = harness();
    h.setProbeOutcome('unreachable');
    h.registry.acquire(['stats']);
    failImmediately(h, PROBE_AFTER_FAILURES);
    await vi.advanceTimersByTimeAsync(0);
    expect(h.authFailures).toBe(0);
    expect(h.manager.state()).toBe('backoff');
    expect(h.manager.indicator()).toBe('reconnecting');
    expect(h.manager.detail()).toBe(UNREACHABLE_DETAIL);
    h.manager.dispose();
  });

  it('keeps backing off on an inconclusive 5xx', async () => {
    const h = harness();
    h.setProbeOutcome('inconclusive');
    h.registry.acquire(['stats']);
    failImmediately(h, PROBE_AFTER_FAILURES);
    await vi.advanceTimersByTimeAsync(0);
    expect(h.authFailures).toBe(0);
    expect(h.manager.state()).toBe('backoff');
    expect(h.manager.detail()).toBeNull();
    h.manager.dispose();
  });

  it('needs PROBE_AFTER_FAILURES fresh failures before the next probe', async () => {
    const h = harness();
    h.setProbeOutcome('inconclusive');
    h.registry.acquire(['stats']);
    failImmediately(h, PROBE_AFTER_FAILURES);
    await vi.advanceTimersByTimeAsync(0);
    expect(h.probes).toBe(1);

    vi.advanceTimersToNextTimer();
    failImmediately(h, PROBE_AFTER_FAILURES - 1);
    await vi.advanceTimersByTimeAsync(0);
    expect(h.probes).toBe(1);

    failImmediately(h, 1);
    await vi.advanceTimersByTimeAsync(0);
    expect(h.probes).toBe(2);
    h.manager.dispose();
  });

  it('attaches the upgrade-refused detail after two valid-session cycles, not one', async () => {
    const h = harness();
    h.setProbeOutcome('session-valid');
    h.registry.acquire(['stats']);

    failImmediately(h, PROBE_AFTER_FAILURES);
    await vi.advanceTimersByTimeAsync(0);
    expect(PROBE_DIAGNOSTIC_CYCLES).toBe(2);
    expect(h.manager.detail()).toBeNull();

    vi.advanceTimersToNextTimer();
    failImmediately(h, PROBE_AFTER_FAILURES);
    await vi.advanceTimersByTimeAsync(0);
    expect(h.manager.detail()).toBe(UPGRADE_REFUSED_DETAIL);
    // Still three states — the detail is text inside `reconnecting`.
    expect(h.manager.indicator()).toBe('reconnecting');
    h.manager.dispose();
  });

  it('does not fire for a close that came after a healthy open', () => {
    const h = harness();
    h.registry.acquire(['stats']);
    for (let i = 0; i < PROBE_AFTER_FAILURES + 2; i += 1) {
      const socket = latest(h.sockets);
      socket.onopen?.();
      vi.advanceTimersByTime(OPEN_STABLE_MS + 1);
      socket.onclose?.();
      vi.advanceTimersByTime(60_000);
    }
    expect(h.probes).toBe(0);
    h.manager.dispose();
  });
});

describe('visibility', () => {
  it('closes only after the grace period', () => {
    const h = harness();
    h.registry.acquire(['stats']);
    const socket = latest(h.sockets);
    socket.onopen?.();

    h.manager.setSuspended(true);
    vi.advanceTimersByTime(HIDDEN_CLOSE_GRACE_MS - 1);
    expect(socket.closedWith).toBeNull();
    vi.advanceTimersByTime(2);
    expect(socket.closedWith).toBe(1000);
    expect(h.manager.state()).toBe('closed');
    h.manager.dispose();
  });

  it('produces no close and no reconnect on a hide/show inside the grace', () => {
    const h = harness();
    h.registry.acquire(['stats']);
    const socket = latest(h.sockets);
    socket.onopen?.();

    h.manager.setSuspended(true);
    vi.advanceTimersByTime(HIDDEN_CLOSE_GRACE_MS / 2);
    h.manager.setSuspended(false);
    vi.advanceTimersByTime(HIDDEN_CLOSE_GRACE_MS * 2);

    expect(socket.closedWith).toBeNull();
    expect(h.sockets).toHaveLength(1);
    expect(h.manager.state()).toBe('open');
    h.manager.dispose();
  });

  it('reconnects and re-sends the union when it becomes visible again', () => {
    const h = harness();
    h.registry.acquire(['stats']);
    latest(h.sockets).onopen?.();

    h.manager.setSuspended(true);
    vi.advanceTimersByTime(HIDDEN_CLOSE_GRACE_MS + 1);
    expect(h.manager.state()).toBe('closed');

    h.manager.setSuspended(false);
    const reopened = latest(h.sockets);
    reopened.onopen?.();
    expect(h.sockets).toHaveLength(2);
    expect(reopened.sent).toEqual(['{"subscribe":["stats"]}']);
    h.manager.dispose();
  });

  it('opens nothing while suspended, whatever a route asks for', () => {
    const h = harness();
    h.manager.setSuspended(true);
    h.registry.acquire(['stats']);
    vi.advanceTimersByTime(120_000);
    expect(h.sockets).toHaveLength(0);
    h.manager.dispose();
  });

  it('opens nothing on becoming visible when no route wants events', () => {
    const h = harness();
    h.manager.setSuspended(true);
    h.manager.setSuspended(false);
    expect(h.sockets).toHaveLength(0);
    h.manager.dispose();
  });

  it('reconnects when the transport died inside the grace and the tab came back', () => {
    const h = harness();
    h.registry.acquire(['stats']);
    const first = latest(h.sockets);
    first.onopen?.();
    vi.advanceTimersByTime(OPEN_STABLE_MS * 5);

    h.manager.setSuspended(true);
    vi.advanceTimersByTime(HIDDEN_CLOSE_GRACE_MS / 6);
    // Wi-Fi power-save drops the connection while the screen is locked. The
    // grace close is still armed, and no backoff is scheduled while suspended.
    first.onclose?.();
    expect(h.manager.state()).toBe('backoff');

    // Back inside the grace window: cancelling the pending close must not be
    // read as "the connection is still there".
    h.manager.setSuspended(false);
    const reopened = latest(h.sockets);
    reopened.onopen?.();

    expect(h.sockets).toHaveLength(2);
    expect(reopened.sent).toEqual(['{"subscribe":["stats"]}']);
    expect(h.manager.state()).toBe('open');
    h.manager.dispose();
  });

  it('does not let a backoff timer fire while suspended', () => {
    const h = harness();
    h.registry.acquire(['stats']);
    vi.advanceTimersByTime(5_000);
    latest(h.sockets).onclose?.();
    expect(h.manager.state()).toBe('backoff');

    h.manager.setSuspended(true);
    vi.advanceTimersByTime(120_000);
    expect(h.sockets).toHaveLength(1);

    h.manager.setSuspended(false);
    expect(h.sockets).toHaveLength(2);
    h.manager.dispose();
  });
});

describe('message dispatch', () => {
  function open(h: Harness): FakeSocket {
    h.registry.acquire(['stats', 'query', 'config_changed', 'list_refreshed']);
    const socket = latest(h.sockets);
    socket.onopen?.();
    return socket;
  }

  it('fans every documented type out to whoever asked for it', () => {
    const h = harness();
    const socket = open(h);
    const seen: string[] = [];
    for (const type of [
      'query',
      'stats',
      'config_changed',
      'list_refreshed',
    ] as const) {
      h.manager.on(type, () => seen.push(type));
    }
    for (const type of [
      'query',
      'stats',
      'config_changed',
      'list_refreshed',
    ] as const) {
      socket.onmessage?.({ data: JSON.stringify({ type, data: {} }) });
    }
    expect(seen).toEqual(['query', 'stats', 'config_changed', 'list_refreshed']);
    h.manager.dispose();
  });

  it('delivers only to the type that was asked for', () => {
    const h = harness();
    const socket = open(h);
    const stats = vi.fn();
    h.manager.on('stats', stats);
    socket.onmessage?.({ data: '{"type":"query","data":{}}' });
    expect(stats).not.toHaveBeenCalled();
    h.manager.dispose();
  });

  it('stops delivering once a listener releases', () => {
    const h = harness();
    const socket = open(h);
    const listener = vi.fn();
    h.manager.on('stats', listener)();
    socket.onmessage?.({ data: '{"type":"stats","data":{}}' });
    expect(listener).not.toHaveBeenCalled();
    h.manager.dispose();
  });

  it('drops a malformed envelope without throwing', () => {
    const h = harness();
    const socket = open(h);
    const listener = vi.fn();
    h.manager.on('stats', listener);
    for (const frame of [
      'not json',
      '[]',
      '"a string"',
      '{"data":{}}',
      '{"type":"stats"}',
      '{"type":"stats","data":[]}',
      '{"type":"stats","data":null}',
      '{"type":"future_event","data":{}}',
    ]) {
      expect(() => socket.onmessage?.({ data: frame })).not.toThrow();
    }
    socket.onmessage?.({ data: new ArrayBuffer(4) });
    expect(listener).not.toHaveBeenCalled();
    expect(h.drops).toHaveLength(9);
    h.manager.dispose();
  });

  it('dispatches a valid envelope carrying fields the types do not describe', () => {
    const h = harness();
    const socket = open(h);
    const listener = vi.fn();
    h.manager.on('stats', listener);
    socket.onmessage?.({
      data: '{"type":"stats","data":{"future_field":1},"extra":true}',
    });
    expect(listener).toHaveBeenCalledWith({ future_field: 1 });
    h.manager.dispose();
  });
});
