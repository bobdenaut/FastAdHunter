import { useState } from 'preact/hooks';
import { logoutAll } from '../../api/auth';
import { Card } from '../../components/card';
import { ConfirmDialog } from '../../components/confirm-dialog';
import { ErrorState } from '../../components/error-state';
import { navigate } from '../../router/router';
import { LOGIN_PATH } from '../../session/session';
import { PasswordDialog } from './password-dialog';
import { RotateKeyDialog } from './rotate-dialog';

/**
 * The three privileged writes, each behind its own dialog.
 *
 * **No masked key value is drawn.** The artboard shows `fah_••••…` beside the
 * Rotate button; no endpoint returns the current key and `GET /config` does not
 * carry it (`ApiConfig` is address, port and tls), so a mask would imply this
 * browser holds something it does not. The label and the button are the whole
 * row.
 */
export function AccessCard() {
  const [dialog, setDialog] = useState<'password' | 'rotate' | 'logout' | null>(
    null,
  );
  const [error, setError] = useState<Error | null>(null);

  const signOutEverywhere = () => {
    setDialog(null);
    setError(null);
    logoutAll()
      .catch((cause: unknown) => {
        setError(cause instanceof Error ? cause : new Error(String(cause)));
      })
      // The secret is rotated server-side either way; landing on the login page
      // is correct even when the response never arrived.
      .finally(() => navigate(LOGIN_PATH, { replace: true }));
  };

  return (
    <Card title="Access" className="access-card">
      <div class="access-row">
        <div>
          <p class="access-title">Dashboard password</p>
          <p class="note">
            An Argon2id hash in <span class="mono">/config/auth-hash</span>, not
            a config key. Changing it signs every session out.
          </p>
        </div>
        <button type="button" class="btn g" onClick={() => setDialog('password')}>
          Change password
        </button>
      </div>

      <div class="access-row">
        <div>
          <p class="access-title">API key</p>
          <p class="note">
            For scripts and non-browser clients; this dashboard uses the session
            cookie. No endpoint returns the current key, so it is not shown here.
          </p>
        </div>
        <button type="button" class="btn g" onClick={() => setDialog('rotate')}>
          Rotate
        </button>
      </div>

      <div class="access-row">
        <div>
          <p class="access-title">Sessions</p>
          <p class="note">
            Rotates the signing secret in{' '}
            <span class="mono">/data/session-secret</span> — every signed-in
            browser is signed out. Signing out normally only clears this
            browser&rsquo;s cookie; the token itself stays valid until it
            expires.
          </p>
        </div>
        <button type="button" class="btn r" onClick={() => setDialog('logout')}>
          Sign out everywhere
        </button>
      </div>

      {error !== null && <ErrorState error={error} />}

      {dialog === 'password' && (
        <PasswordDialog
          onClose={() => setDialog(null)}
          onSignedOut={() => navigate(LOGIN_PATH, { replace: true })}
        />
      )}
      {dialog === 'rotate' && (
        <RotateKeyDialog onClose={() => setDialog(null)} />
      )}
      {dialog === 'logout' && (
        <ConfirmDialog
          title="Sign out everywhere"
          confirmLabel="Sign out everywhere"
          onCancel={() => setDialog(null)}
          onConfirm={signOutEverywhere}
        >
          This rotates the session secret, so every signed-in browser ends
          immediately — this one included. The password is unchanged.
        </ConfirmDialog>
      )}
    </Card>
  );
}
