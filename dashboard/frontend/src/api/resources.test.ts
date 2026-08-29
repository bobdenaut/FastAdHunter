import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setUnauthorizedHandler } from './core';
import { cleanCache } from './cache';
import {
  MEMORY_PERF_FIELDS,
  PERF_FIELDS,
  getHistoryPerf,
  getHistorySummary,
  historyPerfQuery,
  historySummaryQuery,
} from './history';
import { getDebugMemory } from './debug';
import { changePassword, logoutAll } from './auth';
import { postConfig, rotateApiKey } from './config';
import {
  addList,
  deleteList,
  getLists,
  patchList,
  refreshAllLists,
  refreshList,
} from './lists';
import { getStats } from './stats';
import { getClients } from './clients';
import { getConfig } from './config';
import type { DebugMemory, PerfItem, RefreshAllResponse } from './types';

function respond(status: number, body?: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: () => null },
    json: async () => {
      if (body === undefined) throw new Error('no body');
      return body;
    },
  } as unknown as Response;
}

const fetchMock = vi.fn();

function calledWith(): [string, RequestInit] {
  const call = fetchMock.mock.calls[0];
  return [call?.[0] as string, call?.[1] as RequestInit];
}

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  setUnauthorizedHandler(null);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the history query string', () => {
  it('carries only what the caller set, so the server defaults stand', () => {
    expect(historySummaryQuery({ from: '2026-08-01T00:00:00.000Z' })).toBe(
      'from=2026-08-01T00%3A00%3A00.000Z',
    );
  });

  it('names the resolution when the range is daily', () => {
    const query = historySummaryQuery({
      from: '2026-08-01T00:00:00.000Z',
      resolution: 'day',
    });
    expect(query).toContain('resolution=day');
  });

  it('omits `to` rather than sending a client clock', () => {
    expect(historySummaryQuery({ from: 'x' })).not.toContain('to=');
  });

  it('sends `max_points` only when forced, which is how decimation is tested', () => {
    expect(historySummaryQuery({ from: 'x', max_points: 8 })).toContain(
      'max_points=8',
    );
  });

  it('appends the query to the documented path', async () => {
    fetchMock.mockResolvedValue(respond(200, { items: [] }));
    await getHistorySummary({ from: 'x', resolution: 'hour' });
    expect(calledWith()[0]).toBe(
      '/api/v1/history/summary?from=x&resolution=hour',
    );
  });
});

describe('the read-only accessors', () => {
  it('call the documented paths with no body', async () => {
    for (const [accessor, path] of [
      [getStats, '/api/v1/stats'],
      [getClients, '/api/v1/clients'],
      [getConfig, '/api/v1/config'],
      [getLists, '/api/v1/lists'],
    ] as const) {
      fetchMock.mockReset();
      fetchMock.mockResolvedValue(respond(200, {}));
      await accessor();
      const [url, init] = calledWith();
      expect(url).toBe(path);
      expect(init.method).toBe('GET');
      expect(init.body).toBeUndefined();
    }
  });
});

describe('the list mutations', () => {
  it('posts the add body as given, so `url` and `path` stay the caller choice', async () => {
    fetchMock.mockResolvedValue(respond(200, { id: 'local' }));
    await addList({ path: '/data/lists/local.txt', enabled: true });
    const [url, init] = calledWith();
    expect(url).toBe('/api/v1/lists');
    expect(init.method).toBe('POST');
    expect(JSON.parse(String(init.body))).toEqual({
      path: '/data/lists/local.txt',
      enabled: true,
    });
  });

  it('keeps an explicit `refresh_hours: null` in the patch body', async () => {
    fetchMock.mockResolvedValue(respond(200, { id: 'a' }));
    await patchList('a', { refresh_hours: null });
    expect(JSON.parse(String(calledWith()[1].body))).toEqual({
      refresh_hours: null,
    });
  });

  it('escapes an id that would otherwise change the path', async () => {
    fetchMock.mockResolvedValue(respond(204));
    await deleteList('a/b');
    expect(calledWith()[0]).toBe('/api/v1/lists/a%2Fb');
  });

  it('accepts a 204 from DELETE without demanding a body', async () => {
    fetchMock.mockResolvedValue(respond(204));
    await expect(deleteList('a')).resolves.toBeUndefined();
  });

  it('accepts the 202 from a single refresh', async () => {
    fetchMock.mockResolvedValue(respond(202));
    await refreshList('a');
    const [url, init] = calledWith();
    expect(url).toBe('/api/v1/lists/a/refresh');
    expect(init.method).toBe('POST');
  });

  it('posts refresh-all to the collection, not to an id', async () => {
    const body: RefreshAllResponse = { refreshed: 1, failed: 0, results: [] };
    fetchMock.mockResolvedValue(respond(200, body));
    await refreshAllLists();
    expect(calledWith()[0]).toBe('/api/v1/lists/refresh');
  });

  it('returns the per-list outcomes, rejected ones counted as failed', async () => {
    const body: RefreshAllResponse = {
      refreshed: 1,
      failed: 2,
      results: [
        { id: 'oisd', status: 'ok', rules_active_dns: 51234 },
        { id: 'hagezi', status: 'failed', error: 'fetch failed' },
        { id: 'adaway', status: 'rejected', error: 'rejected: collapse' },
      ],
    };
    fetchMock.mockResolvedValue(respond(200, body));
    const result = await refreshAllLists();
    expect(result.refreshed + result.failed).toBe(result.results.length);
    expect(result.results.map((r) => r.status)).toEqual([
      'ok',
      'failed',
      'rejected',
    ]);
  });
});

describe('the perf history query string', () => {
  it('carries `from` and the comma-joined fields, and nothing else', () => {
    expect(
      historyPerfQuery({
        from: '2026-08-01T00:00:00.000Z',
        fields: ['qps', 'latency'],
      }),
    ).toBe('from=2026-08-01T00%3A00%3A00.000Z&fields=qps%2Clatency');
  });

  it('omits `to`, so the server’s own now ends the window', () => {
    expect(historyPerfQuery({ from: 'x', fields: PERF_FIELDS })).not.toContain(
      'to=',
    );
  });

  it('omits `max_points`, so the perf default of 1000 stands', () => {
    // Which is what makes `stride > 1` reachable at 7 d and 30 d, and the
    // decimation footnote something a range actually trips.
    expect(historyPerfQuery({ from: 'x', fields: PERF_FIELDS })).not.toContain(
      'max_points',
    );
  });

  it('asks for exactly the five keys the Performance page draws', () => {
    // An unknown name is a `400`, and a memory key here would be a figure that
    // page has no business rendering. Pinned, not merely typed.
    expect([...PERF_FIELDS]).toEqual([
      'qps',
      'queries_delta',
      'blocked_delta',
      'allowed_delta',
      'latency',
    ]);
  });

  it('asks for exactly the four keys the Memory page draws', () => {
    expect([...MEMORY_PERF_FIELDS]).toEqual([
      'rss_bytes',
      'peak_rss',
      'memory',
      'minor_page_faults',
    ]);
  });

  it('keeps the two field lists disjoint, so neither page pays for the other', () => {
    const shared = PERF_FIELDS.filter((field) =>
      (MEMORY_PERF_FIELDS as readonly string[]).includes(field),
    );
    expect(shared).toEqual([]);
  });

  it('appends the query to the documented path', async () => {
    fetchMock.mockResolvedValue(respond(200, { items: [] }));
    await getHistoryPerf({ from: 'x', fields: ['qps'] });
    expect(calledWith()[0]).toBe('/api/v1/history/perf?from=x&fields=qps');
  });

  it('reads a trimmed row without demanding the dropped keys', () => {
    // A compile-time scenario: `fields` drops keys entirely, so a consumer has
    // to handle absence rather than read a null as a zero.
    const item: PerfItem = { ts: '2026-08-01T00:00:00Z', qps: 12.5 };
    expect(item.latency).toBeUndefined();
    expect(item.queries_delta).toBeUndefined();
  });
});

describe('the cache clean', () => {
  it('posts with no query string by default, keeping the stale window', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await cleanCache(false);
    const [url, init] = calledWith();
    expect(url).toBe('/api/v1/cache/clean');
    expect(init.method).toBe('POST');
    expect(init.body).toBeUndefined();
  });

  it('asks for the stale purge only when the operator chose it', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await cleanCache(true);
    expect(calledWith()[0]).toBe('/api/v1/cache/clean?stale=true');
  });
});

describe('the memory diagnostics read', () => {
  it('calls the documented debug path with no body', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await getDebugMemory();
    const [url, init] = calledWith();
    expect(url).toBe('/api/v1/debug/memory');
    expect(init.method).toBe('GET');
    expect(init.body).toBeUndefined();
  });

  it('reads the two allocator fields as nullable, never as zero', () => {
    // A compile-time scenario: `null` is "the allocator reported nothing", and
    // a consumer that types them `number` renders unavailable as a measurement.
    const memory: Pick<
      DebugMemory,
      'allocator_committed_bytes' | 'allocator_committed_peak_bytes'
    > = {
      allocator_committed_bytes: null,
      allocator_committed_peak_bytes: null,
    };
    expect(memory.allocator_committed_bytes).toBeNull();
  });
});

describe('the config write', () => {
  it('posts the patch verbatim, so only the changed keys travel', async () => {
    fetchMock.mockResolvedValue(
      respond(200, { applied: false, restart_required: true }),
    );
    await postConfig({ dns: { cache: { max_entries: 20000 } } });
    const [url, init] = calledWith();
    expect(url).toBe('/api/v1/config');
    expect(init.method).toBe('POST');
    expect(JSON.parse(String(init.body))).toEqual({
      dns: { cache: { max_entries: 20000 } },
    });
  });

  it('carries no abort signal — a write must land', async () => {
    fetchMock.mockResolvedValue(
      respond(200, { applied: true, restart_required: false }),
    );
    await postConfig({ history: { retention_days: 60 } });
    expect(calledWith()[1].signal).toBeUndefined();
  });

  it('rotates the API key through its own route, with no body', async () => {
    fetchMock.mockResolvedValue(respond(200, { api_key: 'fah_new' }));
    const response = await rotateApiKey();
    const [url, init] = calledWith();
    expect(url).toBe('/api/v1/config/apikey/rotate');
    expect(init.method).toBe('POST');
    expect(init.body).toBeUndefined();
    expect(response.api_key).toBe('fah_new');
  });
});

describe('the two privileged auth writes', () => {
  it('sends the password change under the documented key names', async () => {
    fetchMock.mockResolvedValue(respond(204));
    await changePassword('old-secret', 'a-much-longer-secret');
    const [url, init] = calledWith();
    expect(url).toBe('/api/v1/auth/password');
    expect(JSON.parse(String(init.body))).toEqual({
      current_password: 'old-secret',
      new_password: 'a-much-longer-secret',
    });
  });

  it('does not bounce to login when the current password is wrong', async () => {
    // Its `401` means "wrong current password" on a page that is already
    // signed in; the shared guard would report a typo as an expired session.
    const bounced = vi.fn();
    setUnauthorizedHandler(bounced);
    fetchMock.mockResolvedValue(
      respond(401, { error: { code: 'unauthorized', message: 'no' } }),
    );
    await expect(changePassword('wrong', 'a-much-longer-secret')).rejects.toThrow();
    expect(bounced).not.toHaveBeenCalled();
  });

  it('signs every session out through the revocation route', async () => {
    fetchMock.mockResolvedValue(respond(204));
    await logoutAll();
    const [url, init] = calledWith();
    expect(url).toBe('/api/v1/auth/logout-all');
    expect(init.method).toBe('POST');
    expect(init.body).toBeUndefined();
  });
});
