import { useState } from 'preact/hooks';
import type { Client } from '../../api/types';

/**
 * Rename in place. One round trip, `PUT /api/v1/clients/{ip}` — live in
 * milliseconds, so it neither confirms nor blocks.
 *
 * Clearing the field sends `{"name": null}`, which is how the API spells "back
 * to unnamed"; an empty string would be a client literally named "", and the
 * handler treats a blank as a clear for the same reason.
 */
export function RenameField({
  client,
  busy,
  onSave,
  onCancel,
}: {
  client: Client;
  busy: boolean;
  onSave: (name: string | null) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(client.name ?? '');
  const trimmed = value.trim();
  const next = trimmed === '' ? null : trimmed;

  return (
    <form
      class="rename"
      onSubmit={(event) => {
        event.preventDefault();
        if (!busy) onSave(next);
      }}
    >
      <label class="field-label" for={`rename-${client.ip}`}>
        Name <span class="note">clear it to go back to unnamed</span>
      </label>
      <div class="rename-controls">
        <input
          id={`rename-${client.ip}`}
          class="field-input mono"
          value={value}
          disabled={busy}
          autocomplete="off"
          spellcheck={false}
          onInput={(event) => setValue(event.currentTarget.value)}
        />
        <button type="submit" class="btn" disabled={busy}>
          Save
        </button>
        <button
          type="button"
          class="btn g"
          disabled={busy}
          onClick={onCancel}
        >
          Cancel
        </button>
      </div>
    </form>
  );
}
