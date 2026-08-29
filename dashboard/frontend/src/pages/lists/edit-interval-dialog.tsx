import { useRef, useState } from 'preact/hooks';
import { patchList } from '../../api/lists';
import type { ListItem } from '../../api/types';
import { ErrorState } from '../../components/error-state';
import { useFocusTrap } from '../../components/focus-trap';

/**
 * `PATCH` takes a partial update, so this sends the one field it changes and
 * nothing else. An emptied box sends `refresh_hours: null`, which is a
 * different request from omitting it: `null` clears the per-list override back
 * to the configured default, absent leaves it alone.
 */
export function EditIntervalDialog({
  item,
  onClose,
  onSaved,
}: {
  item: ListItem;
  onClose: () => void;
  onSaved: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const [hours, setHours] = useState(String(item.refresh_hours));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  useFocusTrap(true, () => dialog.current, onClose);

  const submit = (event: Event) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    const trimmed = hours.trim();
    patchList(item.id, {
      refresh_hours: trimmed === '' ? null : Number(trimmed),
    })
      .then(() => {
        onSaved();
        onClose();
      })
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
        aria-label={`Refresh interval for ${item.id}`}
        ref={dialog}
      >
        <h2>Refresh interval</h2>
        <p class="note">
          <span class="mono">{item.id}</span> — how often FastAdHunter refetches
          this source.
        </p>
        <form class="form" onSubmit={submit}>
          <label class="field-label" for="interval-hours">
            Hours <span class="note">— empty clears the override and uses the configured default</span>
          </label>
          <input
            id="interval-hours"
            class="field-input"
            type="number"
            min="1"
            value={hours}
            onInput={(event) =>
              setHours((event.target as HTMLInputElement).value)
            }
          />
          {error !== null && <ErrorState error={error} />}
          <div class="dialog-actions">
            <button type="button" class="btn g" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" class="btn" disabled={busy}>
              {busy ? 'Saving…' : 'Save'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
