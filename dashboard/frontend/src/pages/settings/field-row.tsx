import { formatMiB } from '../../charts/format';
import { LineEditor } from '../../components/line-editor';
import type { FieldMeta } from './metadata';
import type { FieldValue } from './patch';
import { UpstreamServers } from './server-list';
import { useRef, useState } from 'preact/hooks';

/**
 * One config field: its name, its help line, its control, its mutability tag,
 * and the error that belongs to it.
 *
 * **The tag is metadata, never a response field.** `LIVE` and `RESTART` are
 * carried by hand from CONFIGURATION.md §Mutability classes and
 * `config_store.rs`'s `BOOT_KEYS`; `GET /config` says nothing about either, and
 * the two flags `POST /config` answers with describe a write that already
 * happened rather than the class of a key.
 */
export function FieldRow({
  meta,
  value,
  dirty,
  error,
  onChange,
}: {
  meta: FieldMeta;
  value: unknown;
  dirty: boolean;
  error: string | null;
  onChange: (next: FieldValue) => void;
}) {
  const id = `set-${meta.key.replace(/\./g, '-')}`;
  const described = `${id}-help`;

  return (
    <div class={dirty ? 'set-field is-dirty' : 'set-field'}>
      <div class="set-field-name">
        <label class="set-field-label mono" for={id}>
          {meta.label}
        </label>
        <span class={meta.mutability === 'live' ? 'pill good' : 'pill warn'}>
          {meta.mutability === 'live' ? 'live' : 'restart'}
        </span>
        <p class="note" id={described}>
          {meta.help}
        </p>
      </div>
      <div class="set-field-control">
        {value === undefined ? (
          // An older build that has not grown this key yet. A blank control
          // would invite an edit that invents the value.
          <p class="note">
            Not present in this configuration — nothing to edit.
          </p>
        ) : (
          <Control
            id={id}
            described={described}
            meta={meta}
            value={value}
            onChange={onChange}
          />
        )}
        {error !== null && (
          <p class="set-field-error" role="alert">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}

function Control({
  id,
  described,
  meta,
  value,
  onChange,
}: {
  id: string;
  described: string;
  meta: FieldMeta;
  value: unknown;
  onChange: (next: FieldValue) => void;
}) {
  const control = meta.control;

  if (control.kind === 'bool') {
    const on = value === true;
    return (
      <label class={on ? 'switch on' : 'switch'}>
        <input
          id={id}
          type="checkbox"
          checked={on}
          aria-describedby={described}
          aria-label={meta.key}
          onChange={(event) =>
            onChange((event.target as HTMLInputElement).checked)
          }
        />
        <i />
      </label>
    );
  }

  if (control.kind === 'enum') {
    return (
      <select
        id={id}
        class="field-input"
        value={String(value)}
        aria-describedby={described}
        onChange={(event) =>
          onChange((event.target as HTMLSelectElement).value)
        }
      >
        {control.values.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    );
  }

  if (control.kind === 'server-list') {
    return (
      <UpstreamServers
        rows={Array.isArray(value) ? value : []}
        onChange={onChange}
      />
    );
  }

  if (control.kind === 'string-list') {
    return (
      <DestinationList
        id={id}
        label={meta.key}
        entries={Array.isArray(value) ? (value as string[]) : []}
        onChange={onChange}
      />
    );
  }

  if (control.kind === 'int' || control.kind === 'bytes') {
    const numeric = typeof value === 'number' ? value : Number(value);
    return (
      <>
        <input
          id={id}
          class="field-input mono"
          type="number"
          inputMode="numeric"
          min={control.min}
          max={control.max}
          value={String(value)}
          aria-describedby={described}
          onInput={(event) => {
            const raw = (event.target as HTMLInputElement).value;
            // An empty box is not a zero. Nothing is sent for it — the field
            // keeps whatever it last held until a number is typed.
            if (raw.trim() === '') return;
            onChange(Number(raw));
          }}
        />
        {control.kind === 'bytes' && Number.isFinite(numeric) && (
          <span class="note set-field-aside">{formatMiB(numeric)}</span>
        )}
      </>
    );
  }

  return (
    <input
      id={id}
      class="field-input mono"
      type="text"
      spellcheck={false}
      autocapitalize="off"
      value={String(value)}
      aria-describedby={described}
      onInput={(event) => onChange((event.target as HTMLInputElement).value)}
    />
  );
}

/** The committed shape of the field: one trimmed entry per non-empty line. */
export function parseDestinations(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line !== '');
}

/**
 * `egress.allow_destinations` as one entry per line, over the editor the Custom
 * Rules page already uses. It carries no anchors: the server reports the
 * offending entry by its text rather than by a line number, so a band would be
 * pinned to a line nothing said.
 *
 * **The raw buffer is what is edited; the parse happens on the way out.**
 * `LineEditor`'s textarea is fully controlled, so rendering it from
 * `entries.join('\n')` wrote the parsed form back on every keystroke — and the
 * parse drops the trailing empty line a new entry starts on. Pressing Enter to
 * add a second destination was undone before the next character arrived, and a
 * line cleared mid-document collapsed under the caret. The draft is held here
 * and only dropped when the value arriving from above stops agreeing with it,
 * which is a baseline replacement or a Discard rather than an ordinary
 * re-render.
 */
function DestinationList({
  id,
  label,
  entries,
  onChange,
}: {
  id: string;
  label: string;
  entries: readonly string[];
  onChange: (next: readonly string[]) => void;
}) {
  const area = useRef<HTMLTextAreaElement>(null);
  const [draft, setDraft] = useState<string | null>(null);

  const committed = entries.join('\n');
  const shown =
    draft !== null && parseDestinations(draft).join('\n') === committed
      ? draft
      : committed;

  return (
    <div id={id}>
      <LineEditor
        value={shown}
        onInput={(next) => {
          setDraft(next);
          onChange(parseDestinations(next));
        }}
        anchors={[]}
        disabled={false}
        textareaRef={area}
        label={label}
      />
    </div>
  );
}
