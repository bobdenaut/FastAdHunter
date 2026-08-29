import { ApiError, NetworkError } from '../api/core';
import { Icon } from '../shell/icon';

const TITLES: Record<string, string> = {
  bad_request: 'The request was malformed',
  unauthorized: 'Not signed in',
  not_found: 'Not found',
  conflict: 'That conflicts with the current state',
  validation_failed: 'That was rejected',
  rate_limited: 'Too many attempts',
  unavailable: 'Temporarily unavailable',
  internal: 'The API reported an internal error',
};

/**
 * The envelope's `message` is rendered verbatim — it is written for a human and
 * carries the detail (`line 14: invalid rule syntax`) no generic string can.
 * `code` selects the presentation only.
 */
export function ErrorState({ error }: { error: unknown }) {
  const code = error instanceof ApiError ? error.code : null;
  const title =
    code !== null
      ? (TITLES[code] ?? 'The API refused that')
      : error instanceof NetworkError
        ? 'The API did not answer'
        : 'Something went wrong';
  const message = error instanceof Error ? error.message : String(error);

  return (
    <div class="error-state" role="alert">
      <Icon name="warning" size={16} className="warning" />
      <div>
        <p class="error-state-title">{title}</p>
        <p class="error-state-message">{message}</p>
        {code !== null && <span class="error-state-code">{code}</span>}
      </div>
    </div>
  );
}
