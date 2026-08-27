import { useRef, useState } from 'preact/hooks';
import type { Client, ClientPolicyBody, Policy } from '../../api/types';
import { useFocusTrap } from '../../components/focus-trap';
import { DEFAULT_POLICY } from '../../policy/assignment';
import {
  bothOrNeither,
  parseDays,
  parseTimeOfDay,
} from '../../policy/validation';

/**
 * `Clients.dc.html`'s "Editing a client", as the live editor rather than a
 * specimen: address, policy, days and window, with the artboard's own note
 * beneath.
 *
 * Choosing `default` **clears** the assignment — `default` is implicit and
 * never a target you can assign to, so the honest control for "put this device
 * back where everyone else is" is `DELETE /clients/{ip}/policy`.
 *
 * The three validators mirror the config's own. They answer while the operator
 * types; the API is still the guarantee and its `422` is still rendered.
 */
export function AssignDialog({
  client,
  policies,
  busy,
  onAssign,
  onClear,
  onCancel,
}: {
  client: Client;
  policies: readonly Policy[];
  busy: boolean;
  onAssign: (body: ClientPolicyBody) => void;
  onClear: () => void;
  onCancel: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const [policy, setPolicy] = useState(
    policies.some((item) => item.id === client.policy)
      ? client.policy
      : DEFAULT_POLICY,
  );
  const [days, setDays] = useState('');
  const [start, setStart] = useState('');
  const [end, setEnd] = useState('');

  useFocusTrap(true, () => dialog.current, onCancel);

  const daysError = days.trim() === '' ? null : parseDays(days);
  const windowError =
    bothOrNeither(start, end) ??
    (start.trim() === '' ? null : parseTimeOfDay(start)) ??
    (end.trim() === '' ? null : parseTimeOfDay(end));
  const invalid = daysError !== null || windowError !== null;
  const clearing = policy === DEFAULT_POLICY;

  const submit = () => {
    if (busy || invalid) return;
    if (clearing) {
      onClear();
      return;
    }
    const body: ClientPolicyBody = { policy };
    if (days.trim() !== '') body.days = days.trim();
    if (start.trim() !== '' && end.trim() !== '') {
      body.start = start.trim();
      body.end = end.trim();
    }
    onAssign(body);
  };

  return (
    <div class="dialog-scrim">
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-label={`Policy for ${client.ip}`}
        ref={dialog}
      >
        <h2>Policy for {client.ip}</h2>
        <form
          class="form"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <label class="field-label" for="assign-policy">
            Policy
          </label>
          <select
            id="assign-policy"
            class="field-input"
            value={policy}
            disabled={busy}
            onChange={(event) => setPolicy(event.currentTarget.value)}
          >
            <option value={DEFAULT_POLICY}>
              default — clear this address&apos;s assignment
            </option>
            {policies.map((item) => (
              <option key={item.id} value={item.id}>
                {item.name} ({item.id})
              </option>
            ))}
          </select>

          {!clearing && (
            <>
              <label class="field-label" for="assign-days">
                Days <span class="note">blank means every day</span>
              </label>
              <input
                id="assign-days"
                class="field-input mono"
                value={days}
                placeholder="mon-fri"
                disabled={busy}
                autocomplete="off"
                spellcheck={false}
                onInput={(event) => setDays(event.currentTarget.value)}
              />
              {daysError !== null && <p class="field-error">{daysError}</p>}

              <label class="field-label" for="assign-start">
                Window <span class="note">both bounds, or neither</span>
              </label>
              <div class="window-pair">
                <input
                  id="assign-start"
                  class="field-input mono"
                  value={start}
                  placeholder="21:00"
                  disabled={busy}
                  autocomplete="off"
                  onInput={(event) => setStart(event.currentTarget.value)}
                />
                <span aria-hidden="true">→</span>
                <input
                  id="assign-end"
                  class="field-input mono"
                  value={end}
                  placeholder="07:00"
                  aria-label="Window end"
                  disabled={busy}
                  autocomplete="off"
                  onInput={(event) => setEnd(event.currentTarget.value)}
                />
              </div>
              {windowError !== null && <p class="field-error">{windowError}</p>}
            </>
          )}

          <p class="note assign-note">
            Start and end are set together or not at all. Saving replaces any
            assignment this address already had, in whichever policy held it —
            one address, one assignment. Assignments change no rule mask, so
            this applies at once and recompiles nothing.
          </p>

          <div class="dialog-actions">
            <button
              type="button"
              class="btn g"
              disabled={busy}
              onClick={onCancel}
            >
              Cancel
            </button>
            <button type="submit" class="btn" disabled={busy || invalid}>
              {clearing ? 'Clear assignment' : 'Assign'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
