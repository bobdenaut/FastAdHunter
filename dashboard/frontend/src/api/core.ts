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
  error: { code: string; message: string };
}

export class ApiError extends Error {
  /** The documented slug, or whatever unknown one the server sent. New codes
   *  are part of the compatibility contract, so an unrecognized one is kept
   *  rather than rejected. */
  readonly code: string;
  readonly status: number;
  /** Seconds, or `null` when the header was absent or unparseable. */
  readonly retryAfter: number | null;

  constructor(
    status: number,
    code: string,
    message: string,
    retryAfter: number | null,
  ) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.retryAfter = retryAfter;
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

function envelopeOf(payload: unknown, status: number): [string, string] {
  if (typeof payload === 'object' && payload !== null && 'error' in payload) {
    const error = (payload as { error: unknown }).error;
    if (typeof error === 'object' && error !== null) {
      const record = error as Record<string, unknown>;
      const code = typeof record['code'] === 'string' ? record['code'] : '';
      const message =
        typeof record['message'] === 'string' ? record['message'] : '';
      if (code !== '') return [code, message === '' ? code : message];
    }
  }
  // A non-2xx without the documented envelope is still an error; inventing a
  // code for it would be worse than naming the status.
  return ['internal', `HTTP ${status}`];
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
    throw new NetworkError(`${method} ${path} did not reach the server`, {
      cause,
    });
  }

  if (!response.ok) {
    let payload: unknown = null;
    try {
      payload = await response.json();
    } catch {
      // A body that is not JSON changes nothing: the status is the fact.
    }
    const [code, message] = envelopeOf(payload, response.status);
    const error = new ApiError(
      response.status,
      code,
      message,
      parseRetryAfter(response.headers.get('retry-after')),
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
