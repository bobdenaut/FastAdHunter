import { useRef, useState } from 'preact/hooks';
import { ApiError } from '../../api/core';
import { addList } from '../../api/lists';
import type { AddListRequest } from '../../api/types';
import { useFocusTrap } from '../../components/focus-trap';

/**
 * URL **or** mounted path as an explicit choice, not a guess at the string: a
 * path that happens to start with `http` and a URL that happens to look like a
 * path are both things an operator can type, and the API takes one field or the
 * other.
 *
 * A `409` is rendered as what it is. The API sends two of them and they need
 * different actions, so the message shape decides which — a derived-id
 * collision offers a retry with an explicit id prefilled, and a source already
 * configured elsewhere names the list that holds it, because adding a second id
 * over one source is the thing that must not happen.
 */
export function AddListDialog({
  onClose,
  onAdded,
}: {
  onClose: () => void;
  onAdded: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const [kind, setKind] = useState<'url' | 'path'>('url');
  const [source, setSource] = useState('');
  const [id, setId] = useState('');
  const [hours, setHours] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | Error | null>(null);

  useFocusTrap(true, () => dialog.current, onClose);

  const conflict = error instanceof ApiError && error.status === 409;
  const otherList = conflict ? listNamedIn(error.message) : null;
  const idCollision = conflict && otherList === null;

  const submit = (event: Event) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);

    const body: AddListRequest = kind === 'url' ? { url: source } : { path: source };
    if (id.trim() !== '') body.id = id.trim();
    if (hours.trim() !== '') body.refresh_hours = Number(hours);

    addList(body)
      .then(() => {
        onAdded();
        onClose();
      })
      .catch((cause: unknown) => {
        setError(cause instanceof Error ? cause : new Error(String(cause)));
        // A derived-id collision is fixed by naming an id, so the field is
        // filled with what the server derived rather than left blank.
        if (
          cause instanceof ApiError &&
          cause.status === 409 &&
          listNamedIn(cause.message) === null
        ) {
          setId((current) => (current === '' ? derivedIdIn(cause.message) : current));
        }
      })
      .finally(() => setBusy(false));
  };

  return (
    <div class="dialog-scrim">
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Add a list"
        ref={dialog}
      >
        <h2>Add a list</h2>
        <form class="form" onSubmit={submit}>
          <fieldset class="field-set">
            <legend class="field-label">Source</legend>
            <label class="radio">
              <input
                type="radio"
                name="kind"
                checked={kind === 'url'}
                onChange={() => setKind('url')}
              />
              URL
            </label>
            <label class="radio">
              <input
                type="radio"
                name="kind"
                checked={kind === 'path'}
                onChange={() => setKind('path')}
              />
              Mounted path
            </label>
          </fieldset>

          <label class="field-label" for="list-source">
            {kind === 'url' ? 'List URL' : 'Path inside the container'}
          </label>
          <input
            id="list-source"
            class="field-input"
            required
            value={source}
            // No example URL: the postbuild gate forbids an external URL
            // literal in any emitted asset, and a placeholder is not worth
            // weakening that check for.
            placeholder={
              kind === 'url'
                ? 'the list’s full URL, scheme included'
                : '/data/lists/local.txt'
            }
            onInput={(event) =>
              setSource((event.target as HTMLInputElement).value)
            }
          />

          <label class="field-label" for="list-id">
            Id <span class="note">— optional; derived from the source when left blank</span>
          </label>
          <input
            id="list-id"
            class="field-input"
            value={id}
            onInput={(event) => setId((event.target as HTMLInputElement).value)}
          />

          <label class="field-label" for="list-hours">
            Refresh every <span class="note">— hours; blank uses the configured default</span>
          </label>
          <input
            id="list-hours"
            class="field-input"
            type="number"
            min="1"
            value={hours}
            onInput={(event) =>
              setHours((event.target as HTMLInputElement).value)
            }
          />

          {error !== null && (
            <div class="field-error" role="alert">
              {error.message}
              {otherList !== null && (
                <div class="note">
                  That source is already configured as{' '}
                  <span class="mono">{otherList}</span>. Two ids over one source
                  would fetch, cache and compile it twice — change that list
                  rather than adding a second.
                </div>
              )}
              {idCollision && (
                <div class="note">
                  Give the list an explicit id above and try again.
                </div>
              )}
            </div>
          )}

          <div class="dialog-actions">
            <button type="button" class="btn g" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" class="btn" disabled={busy}>
              {busy ? 'Adding…' : 'Add list'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

/** `"<source> is already configured as list <id>"` — the source conflict. */
export function listNamedIn(message: string): string | null {
  return /is already configured as list (\S+)\s*$/.exec(message)?.[1] ?? null;
}

/** `"list <id> already exists…"` — the derived-id conflict. */
export function derivedIdIn(message: string): string {
  return /^list (\S+) already exists/.exec(message)?.[1] ?? '';
}
