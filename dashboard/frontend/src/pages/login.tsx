import { useEffect, useState } from 'preact/hooks';
import { ApiError, NetworkError } from '../api/core';
import { login } from '../api/auth';
import { getHealth } from '../api/health';
import { every } from '../lifecycle/timers';
import { navigate } from '../router/router';
import { clearIntendedPath, intendedPath } from '../session/session';
import { Icon } from '../shell/icon';

export interface Failure {
  message: string;
  /** Seconds to wait, when the API said how long. `null` means there is no
   *  timer to run — including the `503` that never clears on its own. */
  retryAfter: number | null;
}

/**
 * The documented failure set, rendered from the envelope (API.md §Session
 * authentication). The two `401` causes are byte-identical by design, so the
 * page must not claim to tell them apart.
 */
export function failureFor(error: unknown): Failure {
  if (error instanceof NetworkError) {
    return { message: 'The API did not answer.', retryAfter: null };
  }
  if (!(error instanceof ApiError)) {
    return { message: 'Sign-in failed.', retryAfter: null };
  }
  switch (error.status) {
    case 401:
      return { message: 'That password is not right.', retryAfter: null };
    case 429:
      return { message: 'Too many attempts.', retryAfter: error.retryAfter };
    case 503:
      // A `503` with no `Retry-After` is the `api.tls = false` case. `api.tls`
      // is a boot key, so it never clears on its own: no timer, and the reason
      // is said out loud.
      return error.retryAfter === null
        ? {
            message:
              'The API is not serving TLS; sign-in stays unavailable until the operator changes that and restarts.',
            retryAfter: null,
          }
        : {
            message: 'Password verification is saturated.',
            retryAfter: error.retryAfter,
          };
    default:
      return { message: error.message, retryAfter: null };
  }
}

export function Login() {
  const [password, setPassword] = useState('');
  const [failure, setFailure] = useState<Failure | null>(null);
  const [waitSecs, setWaitSecs] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    getHealth(controller.signal)
      .then((health) => setVersion(health.version))
      .catch(() => setVersion(null));
    return () => controller.abort();
  }, []);

  const counting = waitSecs !== null && waitSecs > 0;

  useEffect(() => {
    if (!counting) return;
    return every(1_000, () => {
      setWaitSecs((current) =>
        current === null || current <= 1 ? null : current - 1,
      );
    });
  }, [counting]);

  const blocked = busy || counting;

  return (
    <div class="login">
      <form
        class="login-card"
        onSubmit={(event) => {
          event.preventDefault();
          if (blocked) return;
          setBusy(true);
          setFailure(null);
          login(password)
            .then(() => {
              const next = intendedPath();
              clearIntendedPath();
              setPassword('');
              navigate(next, { replace: true });
            })
            .catch((error: unknown) => {
              const next = failureFor(error);
              setFailure(next);
              setWaitSecs(next.retryAfter);
            })
            .finally(() => setBusy(false));
        }}
      >
        <div class="login-brand">
          <Icon name="brand" size={22} />
          FastAdHunter
        </div>
        <label class="login-label" for="fah-password">
          Password
        </label>
        <input
          id="fah-password"
          class="login-input"
          type="password"
          autocomplete="current-password"
          value={password}
          onInput={(event) =>
            setPassword((event.target as HTMLInputElement).value)
          }
        />
        {failure !== null && (
          <p class="login-error" role="alert">
            {failure.message}
            {counting && ` Try again in ${waitSecs} s.`}
          </p>
        )}
        <button type="submit" class="btn" disabled={blocked}>
          {busy ? 'Signing in…' : 'Sign in'}
        </button>
        {version !== null && <p class="note login-version">v{version}</p>}
      </form>
    </div>
  );
}

export default Login;
