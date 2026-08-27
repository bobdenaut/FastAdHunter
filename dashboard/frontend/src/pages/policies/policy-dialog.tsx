import { useEffect, useRef, useState } from 'preact/hooks';
import { ApiError } from '../../api/core';
import type {
  Assignment,
  CreatePolicyBody,
  ListItem,
  PatchPolicyBody,
  Policy,
} from '../../api/types';
import { ErrorState } from '../../components/error-state';
import { useFocusTrap } from '../../components/focus-trap';
import { parseSelector } from '../../policy/selectors';
import {
  bothOrNeither,
  parseDays,
  parseTimeOfDay,
  validatePolicyId,
} from '../../policy/validation';
import { Icon } from '../../shell/icon';

/** What the page is asked to send, and whether sending it rebuilds the
 *  ruleset. The dialog decides neither the confirmation nor the wait — it
 *  reports the one fact both depend on. */
export interface PolicyDraft {
  create: CreatePolicyBody | null;
  patch: PatchPolicyBody | null;
  recompiles: boolean;
}

interface Row {
  client: string;
  days: string;
  start: string;
  end: string;
}

const NULL_IP = 'null_ip';

function toRow(assignment: Assignment): Row {
  return {
    client: assignment.client,
    days: assignment.days ?? '',
    start: assignment.start ?? '',
    end: assignment.end ?? '',
  };
}

function toAssignment(row: Row): Assignment {
  const assignment: Assignment = { client: row.client.trim() };
  if (row.days.trim() !== '') assignment.days = row.days.trim();
  if (row.start.trim() !== '' && row.end.trim() !== '') {
    assignment.start = row.start.trim();
    assignment.end = row.end.trim();
  }
  return assignment;
}

function rowError(row: Row): string | null {
  const spec = row.client.trim();
  if (spec === '') return 'a client selector is required';
  if (parseSelector(spec) === null) {
    return `"${spec}" is not an address, a prefix or a client name`;
  }
  return (
    (row.days.trim() === '' ? null : parseDays(row.days)) ??
    bothOrNeither(row.start, row.end) ??
    (row.start.trim() === '' ? null : parseTimeOfDay(row.start)) ??
    (row.end.trim() === '' ? null : parseTimeOfDay(row.end))
  );
}

function sameLists(
  left: string[] | null,
  right: string[] | null,
): boolean {
  if (left === null || right === null) return left === right;
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function sameAssignments(
  left: readonly Assignment[],
  right: readonly Assignment[],
): boolean {
  return (
    left.length === right.length &&
    left.every((entry, index) => {
      const other = right[index];
      return (
        other !== undefined &&
        entry.client === other.client &&
        entry.days === other.days &&
        entry.start === other.start &&
        entry.end === other.end
      );
    })
  );
}

/**
 * Create and edit in one form, because the fields are the same set and two
 * copies would drift.
 *
 * **`lists` is a double option and the request builder must honour it**:
 * absent leaves the subset alone, an explicit `null` clears it back to "every
 * enabled list", an array sets it. The same is true of `blocking_mode`, whose
 * only accepted value is `null_ip` — so the control is a two-state override
 * rather than a free list, and clearing it sends a literal `null`.
 *
 * **Only changed fields are sent.** Posting an unchanged subset back would
 * still compare equal server-side and still not recompile, but it makes an
 * assignment edit indistinguishable from a subset edit in a request log.
 */
export function PolicyDialog({
  policy,
  lists,
  busy,
  error,
  suppressed = false,
  onSubmit,
  onCancel,
}: {
  /** `null` creates. */
  policy: Policy | null;
  lists: readonly ListItem[];
  busy: boolean;
  error: Error | null;
  /**
   * A recompile confirmation is stacked over this form. The form stays mounted
   * so cancelling returns to what was typed, but its focus trap steps aside —
   * two live traps means one `Escape` closes both, and the outer one is the
   * work.
   */
  suppressed?: boolean;
  onSubmit: (draft: PolicyDraft) => void;
  onCancel: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const [id, setId] = useState(policy?.id ?? '');
  const [name, setName] = useState(policy?.name ?? '');
  const [subset, setSubset] = useState<string[] | null>(policy?.lists ?? null);
  const [blocking, setBlocking] = useState<string | null>(
    policy?.blocking_mode ?? null,
  );
  const [rows, setRows] = useState<Row[]>(
    (policy?.assignments ?? []).map(toRow),
  );

  useFocusTrap(!suppressed, () => dialog.current, onCancel);

  // A `409` names an id that already exists, so the field that caused it is
  // where the operator has to go next. The typed values are all still here —
  // the dialog is not unmounted on a failure.
  //
  // Gated on `busy` because the response lands before the busy state clears,
  // and a disabled input cannot take focus — measured: the call was a silent
  // no-op and focus stayed on a radio.
  useEffect(() => {
    if (policy !== null || busy) return;
    if (error instanceof ApiError && error.status === 409) {
      document.getElementById('policy-id')?.focus();
    }
  }, [error, policy, busy]);

  const idError = policy === null ? validatePolicyId(id) : null;
  // An empty subset is not "every enabled list" — the engine gives a
  // `lists: []` policy no list at all, so it would block nothing.
  const subsetError =
    subset !== null && subset.length === 0
      ? 'pick at least one list — an empty subset gives this policy no list at all'
      : null;
  const rowErrors = rows.map(rowError);
  const invalid =
    (policy === null && idError !== null) ||
    subsetError !== null ||
    rowErrors.some((entry) => entry !== null);

  const assignments = rows.map(toAssignment);
  const listsChanged =
    policy !== null && !sameLists(policy.lists, subset);

  const submit = () => {
    if (busy || invalid) return;
    if (policy === null) {
      const body: CreatePolicyBody = { id: id.trim() };
      if (name.trim() !== '') body.name = name.trim();
      if (subset !== null) body.lists = subset;
      if (blocking !== null) body.blocking_mode = blocking;
      if (assignments.length > 0) body.assignments = assignments;
      onSubmit({ create: body, patch: null, recompiles: true });
      return;
    }
    const patch: PatchPolicyBody = {};
    if (name.trim() !== '' && name.trim() !== policy.name) {
      patch.name = name.trim();
    }
    if (listsChanged) patch.lists = subset;
    if (blocking !== policy.blocking_mode) patch.blocking_mode = blocking;
    if (!sameAssignments(policy.assignments, assignments)) {
      patch.assignments = assignments;
    }
    onSubmit({ create: null, patch, recompiles: listsChanged });
  };

  const updateRow = (index: number, patch: Partial<Row>) => {
    setRows((current) =>
      current.map((row, position) =>
        position === index ? { ...row, ...patch } : row,
      ),
    );
  };

  return (
    <div class="dialog-scrim">
      <div
        class="dialog dialog-wide"
        role="dialog"
        aria-modal="true"
        aria-label={policy === null ? 'New policy' : `Edit ${policy.id}`}
        ref={dialog}
      >
        <h2>{policy === null ? 'New policy' : `Edit ${policy.id}`}</h2>
        <form
          class="form"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          {policy === null && (
            <>
              <label class="field-label" for="policy-id">
                Id{' '}
                <span class="note">
                  lowercase letters, digits, <span class="mono">.</span>{' '}
                  <span class="mono">_</span> <span class="mono">-</span>
                </span>
              </label>
              <input
                id="policy-id"
                class="field-input mono"
                value={id}
                disabled={busy}
                autocomplete="off"
                spellcheck={false}
                onInput={(event) => setId(event.currentTarget.value)}
              />
              {id !== '' && idError !== null && (
                <p class="field-error">{idError}</p>
              )}
            </>
          )}

          <label class="field-label" for="policy-name">
            Name <span class="note">shown on the card; the id is the key</span>
          </label>
          <input
            id="policy-name"
            class="field-input"
            value={name}
            disabled={busy}
            autocomplete="off"
            onInput={(event) => setName(event.currentTarget.value)}
          />

          <fieldset class="field-set field-set-block">
            <legend>Rule lists</legend>
            <label class="radio">
              <input
                type="radio"
                name="policy-lists"
                checked={subset === null}
                disabled={busy}
                onChange={() => setSubset(null)}
              />
              every enabled list
            </label>
            <label class="radio">
              <input
                type="radio"
                name="policy-lists"
                checked={subset !== null}
                disabled={busy}
                onChange={() => setSubset(policy?.lists ?? [])}
              />
              only these
            </label>
          </fieldset>

          {subset !== null && (
            <div class="subset-picker">
              {lists.length === 0 ? (
                <p class="note">No list is configured to choose from.</p>
              ) : (
                lists.map((list) => (
                  <label class="radio" key={list.id}>
                    <input
                      type="checkbox"
                      checked={subset.includes(list.id)}
                      disabled={busy}
                      onChange={(event) =>
                        setSubset((current) => {
                          const held = current ?? [];
                          return event.currentTarget.checked
                            ? [...held, list.id]
                            : held.filter((entry) => entry !== list.id);
                        })
                      }
                    />
                    <span class="mono">{list.id}</span>
                    {!list.enabled && <span class="note">disabled</span>}
                  </label>
                ))
              )}
              {subsetError !== null && (
                <p class="field-error">{subsetError}</p>
              )}
            </div>
          )}

          <fieldset class="field-set field-set-block">
            <legend>Blocking mode</legend>
            <label class="radio">
              <input
                type="radio"
                name="policy-blocking"
                checked={blocking === null}
                disabled={busy}
                onChange={() => setBlocking(null)}
              />
              inherit the global mode
            </label>
            <label class="radio">
              <input
                type="radio"
                name="policy-blocking"
                checked={blocking === NULL_IP}
                disabled={busy}
                onChange={() => setBlocking(NULL_IP)}
              />
              <span class="mono">null_ip</span>
            </label>
          </fieldset>

          <label class="field-label">
            Assignments{' '}
            <span class="note">
              an address, a prefix such as{' '}
              <span class="mono">192.168.20.0/24</span>, or a client name
            </span>
          </label>
          {rows.map((row, index) => (
            <div class="assignment-editor" key={index}>
              <div class="assignment-editor-row">
                <input
                  class="field-input mono"
                  value={row.client}
                  placeholder="192.168.10.50"
                  aria-label="Client selector"
                  disabled={busy}
                  autocomplete="off"
                  spellcheck={false}
                  onInput={(event) =>
                    updateRow(index, { client: event.currentTarget.value })
                  }
                />
                <input
                  class="field-input mono"
                  value={row.days}
                  placeholder="days"
                  aria-label="Days"
                  disabled={busy}
                  autocomplete="off"
                  onInput={(event) =>
                    updateRow(index, { days: event.currentTarget.value })
                  }
                />
                <input
                  class="field-input mono"
                  value={row.start}
                  placeholder="21:00"
                  aria-label="Window start"
                  disabled={busy}
                  autocomplete="off"
                  onInput={(event) =>
                    updateRow(index, { start: event.currentTarget.value })
                  }
                />
                <input
                  class="field-input mono"
                  value={row.end}
                  placeholder="07:00"
                  aria-label="Window end"
                  disabled={busy}
                  autocomplete="off"
                  onInput={(event) =>
                    updateRow(index, { end: event.currentTarget.value })
                  }
                />
                <button
                  type="button"
                  class="iconbtn danger"
                  aria-label={`Remove assignment ${index + 1}`}
                  disabled={busy}
                  onClick={() =>
                    setRows((current) =>
                      current.filter((_, position) => position !== index),
                    )
                  }
                >
                  <Icon name="trash" size={15} />
                </button>
              </div>
              {rowErrors[index] !== null && (
                <p class="field-error">{rowErrors[index]}</p>
              )}
            </div>
          ))}
          <button
            type="button"
            class="btn g add-assignment"
            disabled={busy}
            onClick={() =>
              setRows((current) => [
                ...current,
                { client: '', days: '', start: '', end: '' },
              ])
            }
          >
            Add assignment
          </button>

          {error !== null && <ErrorState error={error} />}

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
              {policy === null ? 'Create policy' : 'Save changes'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
