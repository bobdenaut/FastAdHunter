/**
 * The one request path. Every resource module derives from it, and no second
 * fetch wrapper exists anywhere in the application: session semantics, the
 * error envelope and `Retry-After` are read in exactly one place.
 */

export type ApiErrorCode =
  | 'bad_request'
  | 'unauthorized'
  | 'not_found'
  | 'conflict'
  | 'validation_failed'
  | 'rate_limited'
  | 'unavailable'
  | 'internal';

export interface ErrorEnvelope {
  /** `details` is optional and endpoint-specific: only `PUT /interception`
   *  sends one today (API.md §Interception). It is carried as `unknown` here
   *  because this module has no business knowing any endpoint's shape — the
   *  resource module that expects one narrows it. */
  error: { code: string; message: string; details?: unknown };
}

export class ApiError extends Error {
  /** The documented slug, or whatever unknown one the server sent. New codes
   *  are part of the compatibility contract, so an unrecognized one is kept
   *  rather than rejected. */
  readonly code: string;
  readonly status: number;
  /** Seconds, or `null` when the header was absent or unparseable. */
  readonly retryAfter: number | null;
  /** The envelope's `details`, verbatim and unnarrowed, or `null` when the
   *  answer carried none. A caller that expects one runtime-checks it; nothing
   *  here parses `message` to recover what `details` already says. */
  readonly details: unknown;

  constructor(
    status: number,
    code: string,
    message: string,
    retryAfter: number | null,
    details: unknown = null,
  ) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.retryAfter = retryAfter;
    this.details = details;
  }

  /**
   * `api.tls = false` answers `503` with no `Retry-After` and never clears on
   * its own — it needs a configuration change and a restart. A client that
   * retries that on a timer retries for ever (API.md §Error format).
   */
  get retryable(): boolean {
    if (this.status === 503) return this.retryAfter !== null;
    return this.status === 429;
  }
}

/** Raised when the request never reached the server. Distinct from `ApiError`,
 *  which means the server answered and said no. */
export class NetworkError extends Error {
  constructor(message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = 'NetworkError';
  }
}

export type UnauthorizedHandler = () => void;

let unauthorized: UnauthorizedHandler | null = null;

/**
 * Installed once by the shell. A page never handles `401`: the guard is here so
 * every route inherits it and none can forget.
 */
export function setUnauthorizedHandler(handler: UnauthorizedHandler | null) {
  unauthorized = handler;
}

/**
 * Whether the API answered the last request that was made. `unknown` until the
 * first one completes.
 *
 * This is an **event-driven** signal with no clock behind it: it moves when a
 * page reads, and nothing polls to keep it fresh. On a route that opens no
 * socket and reads once on mount, `reachable` therefore means "the API
 * answered when this page loaded", not "the API is answering right now" — the
 * four filtering pages are deliberately clock-free (`activeTimers() === 0`),
 * and a heartbeat here would be the timer they refuse.
 *
 * Only a request that never reached the server clears it. A `4xx` or a `5xx`
 * is an answer, so the API is up and said no.
 */
export type ApiReach = 'unknown' | 'reachable' | 'unreachable';

let reach: ApiReach = 'unknown';
const reachListeners = new Set<(next: ApiReach) => void>();

export function apiReach(): ApiReach {
  return reach;
}

export function subscribeApiReach(
  listener: (next: ApiReach) => void,
): () => void {
  reachListeners.add(listener);
  return () => reachListeners.delete(listener);
}

function setReach(next: ApiReach) {
  if (next === reach) return;
  reach = next;
  for (const listener of reachListeners) listener(next);
}

export interface RequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
  body?: unknown;
  signal?: AbortSignal;
  /** `false` on the login route, whose `401` means "wrong password" and must
   *  not bounce the page it is already on. */
  notifyUnauthorized?: boolean;
}

export function parseRetryAfter(raw: string | null): number | null {
  if (raw === null) return null;
  const seconds = Number(raw.trim());
  return Number.isFinite(seconds) && seconds >= 0 ? seconds : null;
}

function envelopeOf(
  payload: unknown,
  status: number,
): [string, string, unknown] {
  if (typeof payload === 'object' && payload !== null && 'error' in payload) {
    const error = (payload as { error: unknown }).error;
    if (typeof error === 'object' && error !== null) {
      const record = error as Record<string, unknown>;
      const code = typeof record['code'] === 'string' ? record['code'] : '';
      const message =
        typeof record['message'] === 'string' ? record['message'] : '';
      if (code !== '') {
        return [code, message === '' ? code : message, record['details'] ?? null];
      }
    }
  }
  // A non-2xx without the documented envelope is still an error; inventing a
  // code for it would be worse than naming the status.
  return ['internal', `HTTP ${status}`, null];
}

export async function request<T>(
  path: string,
  options: RequestOptions = {},
): Promise<T> {
  const { method = 'GET', body, signal, notifyUnauthorized = true } = options;

  const init: RequestInit = {
    method,
    // The dashboard authenticates by cookie and never sends the bearer key.
    credentials: 'same-origin',
  };
  if (signal !== undefined) init.signal = signal;
  if (body !== undefined) {
    init.body = JSON.stringify(body);
    init.headers = { 'content-type': 'application/json' };
  }

  let response: Response;
  try {
    response = await fetch(path, init);
  } catch (cause) {
    if (cause instanceof DOMException && cause.name === 'AbortError') throw cause;
    setReach('unreachable');
    throw new NetworkError(`${method} ${path} did not reach the server`, {
      cause,
    });
  }

  setReach('reachable');

  if (!response.ok) {
    let payload: unknown = null;
    try {
      payload = await response.json();
    } catch {
      // A body that is not JSON changes nothing: the status is the fact.
    }
    const [code, message, details] = envelopeOf(payload, response.status);
    const error = new ApiError(
      response.status,
      code,
      message,
      parseRetryAfter(response.headers.get('retry-after')),
      details,
    );
    if (response.status === 401 && notifyUnauthorized) unauthorized?.();
    throw error;
  }

  // The two documented bodiless successes. `202` is the accepted-but-not-done
  // mutation (`POST /lists/{id}/refresh`), whose outcome arrives as an event
  // rather than in this response; parsing its empty body would throw a syntax
  // error where nothing actually failed.
  if (response.status === 204 || response.status === 202) return undefined as T;

  // Unknown fields are ignored rather than policed: the types describe the
  // documented shape, and the compatibility contract permits additions.
  return (await response.json()) as T;
}
