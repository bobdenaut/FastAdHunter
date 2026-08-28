import { request } from './core';

export const LOGIN_PATH = '/api/v1/auth/login';
export const LOGOUT_PATH = '/api/v1/auth/logout';
export const LOGOUT_ALL_PATH = '/api/v1/auth/logout-all';
export const PASSWORD_PATH = '/api/v1/auth/password';

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

/**
 * The only revocation. It rotates `/data/session-secret`, so every session
 * everywhere ends immediately — this browser's included, which is why the
 * caller navigates to the login page on success.
 */
export function logoutAll(): Promise<void> {
  return request<void>(LOGOUT_ALL_PATH, { method: 'POST' });
}

/**
 * `204`, cookie cleared, every session dead — the caller's included, and no
 * replacement cookie is issued.
 *
 * `notifyUnauthorized: false` because a `401` here means "the current password
 * is wrong", which the dialog says in place. Letting the shared guard bounce to
 * `/login` would report a typo as an expired session.
 */
export function changePassword(
  current: string,
  next: string,
): Promise<void> {
  return request<void>(PASSWORD_PATH, {
    method: 'POST',
    body: { current_password: current, new_password: next },
    notifyUnauthorized: false,
  });
}
