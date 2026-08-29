import type { Config } from '../../api/types';
import { Card } from '../../components/card';
import { FieldRow } from './field-row';
import type { SectionMeta } from './metadata';
import { fieldValue, type Edits, type FieldValue } from './patch';

/**
 * One `[section]` card. The rules card carries one extra row the form does not
 * own — see `RuleListsRow`.
 */
export function SectionCard({
  section,
  baseline,
  edits,
  errors,
  onChange,
}: {
  section: SectionMeta;
  baseline: Config | null;
  edits: Edits;
  errors: ReadonlyMap<string, string>;
  onChange: (key: string, value: FieldValue) => void;
}) {
  return (
    <Card
      title={<span class="mono">[{section.id}]</span>}
      className={`set-section set-section-${section.id.replace(/\./g, '-')}`}
    >
      {/* The section note is prose and belongs in the body. In the title bar's
          `secondary` slot it cannot shrink — measured at 900 px, `[egress]`'s
          note stretched its card header to 1,561 px and pushed the whole page
          676 px sideways. */}
      <p class="note set-section-note">{section.note}</p>
      {section.fields.map((field) => (
        <FieldRow
          key={field.key}
          meta={field}
          value={fieldValue(baseline, edits, field.key)}
          dirty={Object.hasOwn(edits, field.key)}
          error={errors.get(field.key) ?? null}
          onChange={(value) => onChange(field.key, value)}
        />
      ))}
      {section.id === 'rules' && <RuleListsRow baseline={baseline} />}
    </Card>
  );
}

/**
 * D1 — the list count, read from the same `GET /config` response and printed
 * rather than edited.
 *
 * `rules.lists` and `policies` are `422` on this endpoint by design: each has
 * exactly one writer, on its own page, which applies the change live and writes
 * the file back itself. Accepting them here as well would give one piece of
 * state two writers with no reconciliation, and the later write would silently
 * erase the earlier one.
 */
function RuleListsRow({ baseline }: { baseline: Config | null }) {
  const lists = baseline?.rules.lists;
  return (
    <div class="set-field">
      <div class="set-field-name">
        <span class="set-field-label mono">lists</span>
        <span class="pill neutral">elsewhere</span>
        <p class="note">Managed on the Lists page.</p>
      </div>
      <div class="set-field-control">
        <p class="note">
          {lists === undefined
            ? 'Not read yet'
            : `${lists.length} ${lists.length === 1 ? 'list' : 'lists'} — not editable here`}
          <span class="footnote-line">
            The API answers <span class="mono">422</span> for this key.{' '}
            <span class="mono">rules.lists</span> and{' '}
            <span class="mono">policies</span> have exactly one writer each — the
            Lists and Policies pages, which apply a change live and persist it
            themselves. Accepting them here too would give one piece of state two
            writers, and the later write would silently erase the earlier one.
          </span>
        </p>
      </div>
    </div>
  );
}
