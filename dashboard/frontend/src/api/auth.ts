import { request } from './core';

export const LOGIN_PATH = '/api/v1/auth/login';
export const LOGOUT_PATH = '/api/v1/auth/logout';

/**
 * `204` and a `Set-Cookie` are the entire result — there is no response body,
 * and the cookie is `HttpOnly`, so nothing is stored client-side.
 *
 * `notifyUnauthorized: false`: a `401` here means the password was wrong, on a
 * page that is already the login page.
 */
export function login(password: string, signal?: AbortSignal): Promise<void> {
  return request<void>(LOGIN_PATH, {
    method: 'POST',
    body: { password },
    notifyUnauthorized: false,
    ...(signal === undefined ? {} : { signal }),
  });
}

/** Client-side only: the token stays valid until its expiry (API.md). */
export function logout(signal?: AbortSignal): Promise<void> {
  return request<void>(LOGOUT_PATH, {
    method: 'POST',
    ...(signal === undefined ? {} : { signal }),
  });
}
