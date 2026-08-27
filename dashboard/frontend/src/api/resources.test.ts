import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setUnauthorizedHandler } from './core';
import { getHistorySummary, historySummaryQuery } from './history';
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
import type { RefreshAllResponse } from './types';

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
