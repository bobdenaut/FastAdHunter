import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError, NetworkError, setUnauthorizedHandler } from '../api/core';
import { PROBE_PATH, classifyProbe, runProbe, sendsToLogin } from './probe';

const fetchMock = vi.fn();

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

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  setUnauthorizedHandler(null);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('classification', () => {
  it('reads 401 as an expired or invalid session', () => {
    expect(classifyProbe(new ApiError(401, 'unauthorized', 'no', null), true))
      .toBe('session-expired');
  });

  it('reads 2xx as a valid session and nothing more', () => {
    expect(classifyProbe(null, false)).toBe('session-valid');
  });

  it('reads 5xx as inconclusive', () => {
    expect(classifyProbe(new ApiError(500, 'internal', 'boom', null), true))
      .toBe('inconclusive');
    expect(classifyProbe(new ApiError(503, 'unavailable', 'busy', 1), true))
      .toBe('inconclusive');
  });

  it('reads no response at all as unreachable', () => {
    expect(classifyProbe(new NetworkError('gone'), true)).toBe('unreachable');
  });

  it('reads anything else as inconclusive', () => {
    expect(classifyProbe(new Error('?'), true)).toBe('inconclusive');
  });

  it('sends the user to login for an expired session and for nothing else', () => {
    expect(sendsToLogin('session-expired')).toBe(true);
    expect(sendsToLogin('session-valid')).toBe(false);
    expect(sendsToLogin('inconclusive')).toBe(false);
    expect(sendsToLogin('unreachable')).toBe(false);
  });
});

describe('the probe itself', () => {
  it('is one request to the documented path, never retried', async () => {
    fetchMock.mockResolvedValue(respond(200, []));
    await expect(runProbe()).resolves.toBe('session-valid');
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0]?.[0]).toBe(PROBE_PATH);
  });

  it('issues exactly one request when it fails, too', async () => {
    fetchMock.mockResolvedValue(
      respond(401, { error: { code: 'unauthorized', message: 'no' } }),
    );
    await expect(runProbe()).resolves.toBe('session-expired');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('does not fire the shell guard — the manager decides what a 401 means', async () => {
    const guard = vi.fn();
    setUnauthorizedHandler(guard);
    fetchMock.mockResolvedValue(
      respond(401, { error: { code: 'unauthorized', message: 'no' } }),
    );
    await runProbe();
    expect(guard).not.toHaveBeenCalled();
  });

  it('reports an unreachable server without throwing', async () => {
    fetchMock.mockRejectedValue(new TypeError('failed to fetch'));
    await expect(runProbe()).resolves.toBe('unreachable');
  });
});
