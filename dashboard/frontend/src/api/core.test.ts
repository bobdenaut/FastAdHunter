import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  ApiError,
  NetworkError,
  parseRetryAfter,
  request,
  setUnauthorizedHandler,
} from './core';
import { getCache } from './cache';
import { getHealth } from './health';
import { login, logout } from './auth';
import { getTelemetry } from './telemetry';

interface StubResponse {
  status: number;
  body?: unknown;
  headers?: Record<string, string>;
}

function respond({ status, body, headers = {} }: StubResponse): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: (name: string) => headers[name.toLowerCase()] ?? null },
    json: async () => {
      if (body === undefined) throw new Error('no body');
      return body;
    },
  } as unknown as Response;
}

const fetchMock = vi.fn();

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  setUnauthorizedHandler(null);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('request', () => {
  it('sends the session cookie and never a bearer key', async () => {
    fetchMock.mockResolvedValue(respond({ status: 200, body: { a: 1 } }));
    await request('/api/v1/cache');
    const init = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(init.credentials).toBe('same-origin');
    expect(init.headers).toBeUndefined();
  });

  it('returns undefined on 204 without reading a body', async () => {
    fetchMock.mockResolvedValue(respond({ status: 204 }));
    await expect(request('/api/v1/auth/logout', { method: 'POST' }))
      .resolves.toBeUndefined();
  });

  it('ignores fields the types do not describe', async () => {
    fetchMock.mockResolvedValue(
      respond({ status: 200, body: { entries: 1, future_field: 'x' } }),
    );
    await expect(getCache()).resolves.toMatchObject({ entries: 1 });
  });

  it('parses the documented error envelope', async () => {
    fetchMock.mockResolvedValue(
      respond({
        status: 422,
        body: { error: { code: 'validation_failed', message: 'line 14' } },
      }),
    );
    const error = await request('/x').catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({
      status: 422,
      code: 'validation_failed',
      message: 'line 14',
    });
  });

  it('keeps an unknown code rather than rejecting it', async () => {
    fetchMock.mockResolvedValue(
      respond({
        status: 418,
        body: { error: { code: 'teapot_overheated', message: 'no' } },
      }),
    );
    const error = (await request('/x').catch((e: unknown) => e)) as ApiError;
    expect(error.code).toBe('teapot_overheated');
  });

  it('names the status when the body is not the documented envelope', async () => {
    fetchMock.mockResolvedValue(respond({ status: 500 }));
    const error = (await request('/x').catch((e: unknown) => e)) as ApiError;
    expect(error.code).toBe('internal');
    expect(error.message).toBe('HTTP 500');
  });

  it('reports a request that never reached the server as a NetworkError', async () => {
    fetchMock.mockRejectedValue(new TypeError('failed to fetch'));
    await expect(request('/x')).rejects.toBeInstanceOf(NetworkError);
  });

  it('rethrows an abort as an abort', async () => {
    fetchMock.mockRejectedValue(
      new DOMException('aborted', 'AbortError'),
    );
    const error = await request('/x').catch((e: unknown) => e);
    expect(error).toBeInstanceOf(DOMException);
  });
});

describe('Retry-After', () => {
  it('parses seconds and tolerates absence', () => {
    expect(parseRetryAfter('30')).toBe(30);
    expect(parseRetryAfter(' 1 ')).toBe(1);
    expect(parseRetryAfter(null)).toBeNull();
    expect(parseRetryAfter('Wed, 21 Oct 2026 07:28:00 GMT')).toBeNull();
    expect(parseRetryAfter('-5')).toBeNull();
  });

  it('marks a 503 without Retry-After non-retryable', async () => {
    fetchMock.mockResolvedValue(
      respond({
        status: 503,
        body: { error: { code: 'unavailable', message: 'tls off' } },
      }),
    );
    const error = (await login('x').catch((e: unknown) => e)) as ApiError;
    expect(error.retryAfter).toBeNull();
    expect(error.retryable).toBe(false);
  });

  it('marks a 503 with Retry-After retryable', async () => {
    fetchMock.mockResolvedValue(
      respond({
        status: 503,
        body: { error: { code: 'unavailable', message: 'saturated' } },
        headers: { 'retry-after': '1' },
      }),
    );
    const error = (await login('x').catch((e: unknown) => e)) as ApiError;
    expect(error.retryAfter).toBe(1);
    expect(error.retryable).toBe(true);
  });
});

describe('the 401 guard', () => {
  it('reaches the guard exactly once per request, then rethrows', async () => {
    const guard = vi.fn();
    setUnauthorizedHandler(guard);
    fetchMock.mockResolvedValue(
      respond({
        status: 401,
        body: { error: { code: 'unauthorized', message: 'no' } },
      }),
    );
    await expect(getTelemetry()).rejects.toBeInstanceOf(ApiError);
    expect(guard).toHaveBeenCalledTimes(1);
  });

  it('is not fired by a wrong password on the login route', async () => {
    const guard = vi.fn();
    setUnauthorizedHandler(guard);
    fetchMock.mockResolvedValue(
      respond({
        status: 401,
        body: { error: { code: 'unauthorized', message: 'no' } },
      }),
    );
    await expect(login('wrong')).rejects.toBeInstanceOf(ApiError);
    expect(guard).not.toHaveBeenCalled();
  });

  it('is not fired by an unauthenticated /health read', async () => {
    const guard = vi.fn();
    setUnauthorizedHandler(guard);
    fetchMock.mockResolvedValue(
      respond({
        status: 401,
        body: { error: { code: 'unauthorized', message: 'no' } },
      }),
    );
    await expect(getHealth()).rejects.toBeInstanceOf(ApiError);
    expect(guard).not.toHaveBeenCalled();
  });
});

describe('resource modules', () => {
  it('call the documented paths', async () => {
    fetchMock.mockResolvedValue(respond({ status: 200, body: {} }));
    await getHealth();
    await getTelemetry();
    await getCache();
    fetchMock.mockResolvedValue(respond({ status: 204 }));
    await login('secret');
    await logout();
    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual([
      '/health',
      '/api/v1/telemetry',
      '/api/v1/cache',
      '/api/v1/auth/login',
      '/api/v1/auth/logout',
    ]);
  });

  it('sends the password as the documented body', async () => {
    fetchMock.mockResolvedValue(respond({ status: 204 }));
    await login('secret');
    const init = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(init.method).toBe('POST');
    expect(init.body).toBe('{"password":"secret"}');
  });

  it('passes an abort signal through', async () => {
    fetchMock.mockResolvedValue(respond({ status: 200, body: {} }));
    const controller = new AbortController();
    await getTelemetry(controller.signal);
    const init = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(init.signal).toBe(controller.signal);
  });
});
