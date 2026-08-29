import { setUnauthorizedHandler } from '../api/core';
import { currentPath, navigate } from '../router/router';

/**
 * There is no session-introspection endpoint, and that is the design. The
 * cookie is `HttpOnly` and unreadable from JS, so nothing is stored
 * client-side: state is inferred from what the API answers.
 *
 * At app start the shell assumes it is signed in and lets the first `401`
 * correct it. A boot probe would be one request every load for a state the
 * cookie already answers.
 */

export const LOGIN_PATH = '/login';

let returnTo: string | null = null;

/** Where to land after a successful sign-in. `/login` is never kept. */
export function intendedPath(): string {
  return returnTo ?? '/';
}

export function clearIntendedPath(): void {
  returnTo = null;
}

/**
 * The one `401` handler, installed once by the shell. A page never handles
 * `401`: the guard is here so every route inherits it and none can forget.
 */
export function endSession(): void {
  const path = currentPath();
  if (path === LOGIN_PATH) return;
  returnTo = path;
  navigate(LOGIN_PATH, { replace: true });
}

export function installSessionGuard(): () => void {
  setUnauthorizedHandler(endSession);
  return () => setUnauthorizedHandler(null);
}
