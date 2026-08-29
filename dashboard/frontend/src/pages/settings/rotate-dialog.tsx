import { useRef, useState } from 'preact/hooks';
import { rotateApiKey } from '../../api/config';
import { ErrorState } from '../../components/error-state';
import { useFocusTrap } from '../../components/focus-trap';

/**
 * **The warning comes before the confirmation, not after it.** The new key is
 * returned once and the old one stops working immediately, so anything still
 * carrying it — a script, a monitor — breaks the moment this is confirmed. That
 * has to be readable while the decision is still reversible.
 *
 * The key is rendered and never stored: no `localStorage`, no module variable
 * outliving this component. Closing the dialog is what loses it, and the dialog
 * says so.
 */
export function RotateKeyDialog({ onClose }: { onClose: () => void }) {
  const dialog = useRef<HTMLDivElement>(null);
  const [busy, setBusy] = useState(false);
  const [key, setKey] = useState<string | null>(null);
  const [error, setError] = useState<Error | null>(null);
  const [copied, setCopied] = useState(false);

  useFocusTrap(true, () => dialog.current, onClose);

  const rotate = () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    rotateApiKey()
      .then((response) => setKey(response.api_key))
      .catch((cause: unknown) => {
        setError(cause instanceof Error ? cause : new Error(String(cause)));
      })
      .finally(() => setBusy(false));
  };

  return (
    <div class="dialog-scrim">
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Rotate the API key"
        ref={dialog}
      >
        <h2>Rotate API key</h2>
        {key === null ? (
          <>
            <p class="note">
              The new key is <b>shown once</b> and cannot be read again — no
              endpoint returns the current key. The old key stops working{' '}
              <b>immediately</b>, so anything still using it — a script, a
              monitor — breaks until it is updated.
            </p>
            <p class="note">
              This dashboard is unaffected: it authenticates with the session
              cookie, not with the key.
            </p>
            {error !== null && <ErrorState error={error} />}
            <div class="dialog-actions">
              <button type="button" class="btn g" onClick={onClose}>
                Cancel
              </button>
              <button
                type="button"
                class="btn"
                disabled={busy}
                onClick={rotate}
              >
                {busy ? 'Rotating…' : 'Rotate the key'}
              </button>
            </div>
          </>
        ) : (
          <>
            <p class="note">
              Copy it now. It is not stored anywhere in this browser and closing
              this dialog loses it.
            </p>
            <p class="mono api-key" data-testid="rotated-key">
              {key}
            </p>
            <div class="dialog-actions">
              <button
                type="button"
                class="btn g"
                onClick={() => {
                  void navigator.clipboard
                    ?.writeText(key)
                    .then(() => setCopied(true))
                    .catch(() => setCopied(false));
                }}
              >
                {copied ? 'Copied' : 'Copy'}
              </button>
              <button type="button" class="btn" onClick={onClose}>
                Done
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
