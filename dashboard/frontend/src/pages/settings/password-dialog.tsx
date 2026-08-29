import { useRef, useState } from 'preact/hooks';
import { ApiError } from '../../api/core';
import { changePassword } from '../../api/auth';
import { useFocusTrap } from '../../components/focus-trap';

/** API.md: `422 validation_failed` below twelve characters. Mirrored here so a
 *  short password costs no round trip; the server stays the authority. */
const MIN_PASSWORD = 12;

/**
 * The current password is required, and that is the point: it is an explicit
 * reauthentication barrier for a privileged operation, and the last one
 * standing in front of an unattended signed-in browser.
 *
 * On success **every session dies, this one included** — the route rotates the
 * session secret before replacing the hash, and issues no replacement cookie.
 * So there is nothing to return to and the dialog says so before navigating.
 */
export function PasswordDialog({
  onClose,
  onSignedOut,
}: {
  onClose: () => void;
  onSignedOut: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [repeat, setRepeat] = useState('');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  // Escape means "leave this dialog", and once the rotation has landed the only
  // honest way to leave it is the sign-out: this browser's cookie was revoked
  // by the request that succeeded, so dismissing the panel to the form behind it
  // would leave the operator reading a page whose session is already gone. The
  // trap reads this through a ref, so switching it as `done` flips is safe.
  useFocusTrap(true, () => dialog.current, done ? onSignedOut : onClose);

  const submit = (event: Event) => {
    event.preventDefault();
    if (busy || done) return;
    if (next !== repeat) {
      setMessage('The two new passwords do not match.');
      return;
    }
    if (next.length < MIN_PASSWORD) {
      setMessage(`The new password must be at least ${MIN_PASSWORD} characters.`);
      return;
    }
    setBusy(true);
    setMessage(null);
    changePassword(current, next)
      // The navigation is **not** fired here. `onSignedOut()` in the same tick
      // as `setDone(true)` left `/login` painted before the done panel ever
      // rendered, so the sentence this dialog exists to show — that every
      // session is gone and why — was unreachable. The operator dismisses it,
      // which is also the only acknowledgement that the rotation happened.
      .then(() => setDone(true))
      .catch((cause: unknown) => {
        setMessage(describe(cause));
      })
      .finally(() => setBusy(false));
  };

  return (
    <div class="dialog-scrim">
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Change the dashboard password"
        ref={dialog}
      >
        <h2>Change password</h2>
        {done ? (
          <>
            <p class="note">
              Every session was signed out, this one included — the session
              secret is rotated before the new hash is written, and no
              replacement cookie is issued. Sign in again with the new password.
            </p>
            <div class="dialog-actions">
              <button
                type="button"
                class="btn"
                onClick={onSignedOut}
                ref={(node) => node?.focus()}
              >
                Sign in again
              </button>
            </div>
          </>
        ) : (
          <>
            <p class="note">
              The current password is required. On success every signed-in
              browser is signed out, this one included.
            </p>
            <form class="form" onSubmit={submit}>
              <label class="field-label" for="pw-current">
                Current password
              </label>
              <input
                id="pw-current"
                class="field-input"
                type="password"
                autocomplete="current-password"
                value={current}
                onInput={(event) =>
                  setCurrent((event.target as HTMLInputElement).value)
                }
              />
              <label class="field-label" for="pw-next">
                New password{' '}
                <span class="note">— at least {MIN_PASSWORD} characters</span>
              </label>
              <input
                id="pw-next"
                class="field-input"
                type="password"
                autocomplete="new-password"
                value={next}
                onInput={(event) =>
                  setNext((event.target as HTMLInputElement).value)
                }
              />
              <label class="field-label" for="pw-repeat">
                New password again
              </label>
              <input
                id="pw-repeat"
                class="field-input"
                type="password"
                autocomplete="new-password"
                value={repeat}
                onInput={(event) =>
                  setRepeat((event.target as HTMLInputElement).value)
                }
              />
              {message !== null && (
                <p class="field-error" role="alert">
                  {message}
                </p>
              )}
              <div class="dialog-actions">
                <button type="button" class="btn g" onClick={onClose}>
                  Cancel
                </button>
                <button type="submit" class="btn" disabled={busy}>
                  {busy ? 'Changing…' : 'Change password'}
                </button>
              </div>
            </form>
          </>
        )}
      </div>
    </div>
  );
}

/**
 * The four documented failures, each said in its own words. A `401` here is a
 * wrong current password rather than an expired session — which is why the
 * request opts out of the shared unauthorized guard.
 */
function describe(cause: unknown): string {
  if (!(cause instanceof ApiError)) {
    return cause instanceof Error ? cause.message : String(cause);
  }
  if (cause.status === 401) return 'The current password is wrong.';
  const wait =
    cause.retryAfter === null ? '' : ` Try again in ${cause.retryAfter} s.`;
  if (cause.status === 429) return `Too many password attempts.${wait}`;
  if (cause.status === 503) {
    return cause.retryable
      ? `Password verification is saturated.${wait}`
      : `${cause.message} This one does not clear on its own.`;
  }
  return cause.message;
}
