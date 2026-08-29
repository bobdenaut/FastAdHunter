import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { request, setUnauthorizedHandler } from '../api/core';
import {
  LOGIN_PATH,
  clearIntendedPath,
  endSession,
  installSessionGuard,
  intendedPath,
} from './session';

let path = '/';
const pushed: Array<{ path: string; replace: boolean }> = [];
const fetchMock = vi.fn();

function unauthorized(): Response {
  return {
    ok: false,
    status: 401,
    headers: { get: () => null },
    json: async () => ({ error: { code: 'unauthorized', message: 'no' } }),
  } as unknown as Response;
}

beforeEach(() => {
  path = '/';
  pushed.length = 0;
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  vi.stubGlobal('window', {
    location: {
      get pathname() {
        return path;
      },
    },
    history: {
      pushState: (_s: unknown, _t: string, next: string) => {
        path = next;
        pushed.push({ path: next, replace: false });
      },
      replaceState: (_s: unknown, _t: string, next: string) => {
        path = next;
        pushed.push({ path: next, replace: true });
      },
    },
    scrollTo: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
  });
  clearIntendedPath();
});

afterEach(() => {
  setUnauthorizedHandler(null);
  vi.unstubAllGlobals();
});

describe('the session guard', () => {
  it('returns to login and keeps the attempted path', () => {
    path = '/cache';
    endSession();
    expect(pushed).toEqual([{ path: LOGIN_PATH, replace: true }]);
    expect(intendedPath()).toBe('/cache');
  });

  it('does not bounce a page that is already the login page', () => {
    path = LOGIN_PATH;
    endSession();
    expect(pushed).toEqual([]);
  });

  it('defaults the post-login destination to the Dashboard', () => {
    expect(intendedPath()).toBe('/');
  });

  it('turns any authenticated 401 into a return to login', async () => {
    path = '/upstreams';
    const uninstall = installSessionGuard();
    fetchMock.mockResolvedValue(unauthorized());
    await request('/api/v1/telemetry').catch(() => undefined);
    uninstall();
    expect(pushed).toEqual([{ path: LOGIN_PATH, replace: true }]);
  });

  it('stops doing so once uninstalled', async () => {
    path = '/upstreams';
    installSessionGuard()();
    fetchMock.mockResolvedValue(unauthorized());
    await request('/api/v1/telemetry').catch(() => undefined);
    expect(pushed).toEqual([]);
  });
});
