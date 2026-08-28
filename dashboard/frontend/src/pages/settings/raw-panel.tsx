import type { Config } from '../../api/types';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { FIELDS } from './metadata';

/**
 * **All settings — the whole `GET /api/v1/config` response, read-only.**
 *
 * A hand-written form is a subset by construction: a key the API gains is
 * invisible until someone adds a field for it, and silent omission is the
 * failure mode — an operator cannot tell "not exposed here" from "not set".
 * This panel removes that ambiguity without generating anything, and its
 * presence is what makes the curated-form decision safe (IA §All settings).
 *
 * Read-only is deliberate. Editing an arbitrary key needs the type, the bounds
 * and the mutability class the curated fields carry by hand; an edit box
 * without them would invite a `422` the UI could not explain.
 */
export function RawPanel({ config }: { config: Config | null }) {
  if (config === null) {
    return (
      <Card title="All settings" className="raw-panel">
        <EmptyState title="Not read yet" />
      </Card>
    );
  }

  const rows = flatten(redact(config));
  const modelled = new Set(FIELDS.map((field) => field.key));
  const unmodelled = rows.filter(
    (row) => !modelled.has(row.key) && !row.key.startsWith('rules.lists'),
  ).length;

  return (
    <Card
      title="All settings"
      secondary="read-only · GET /api/v1/config"
      className="raw-panel"
    >
      <p class="note">
        Everything the effective configuration holds, whether or not the form
        above models it —{' '}
        {unmodelled === 0
          ? 'every key here has a field above'
          : `${unmodelled} of these keys have no field above`}
        . Values only: the response carries no bounds, no enums and no mutability
        classes, so nothing here can be edited without inventing them.
      </p>
      <div class="raw-rows">
        {rows.map((row) => (
          <div class="raw-row" key={row.key}>
            <span class="mono">{row.key}</span>
            <span class="mono">{row.value}</span>
          </div>
        ))}
        {config.policies === undefined && (
          // `skip_serializing_if = "Vec::is_empty"` drops the key entirely when
          // no policy is configured, so the zero-config case shows nothing at
          // all — which is exactly the ambiguity this panel exists to remove.
          <div class="raw-row" key="policies">
            <span class="mono">policies</span>
            <span class="note">none configured</span>
          </div>
        )}
      </div>
      <p class="note raw-foot">
        Auth material never appears here:{' '}
        <span class="mono">GET /api/v1/config</span> omits it rather than
        redacting it — the Argon2id password hash and the session secret are not
        part of the configuration tree at all. This panel drops any{' '}
        <span class="mono">auth</span> key it is handed anyway, as a second
        check rather than a trust.
      </p>
    </Card>
  );
}

/**
 * The second auth check. `p5-04` guarantees the endpoint omits `auth.*`; a
 * password hash printed into a browser is offline-crackable material, and "it
 * is only a hash" is not a reason to publish it — so this panel drops the key
 * rather than assuming it can never arrive.
 */
function redact(config: Config): Record<string, unknown> {
  const { auth: _dropped, ...rest } = config as Config & { auth?: unknown };
  return rest as Record<string, unknown>;
}

interface RawRow {
  key: string;
  value: string;
}

/** Dotted leaves, in the response's own key order. An array is one leaf and is
 *  printed as its JSON — the merge replaces arrays wholesale, so a per-element
 *  key would name something no patch can address. */
function flatten(value: unknown, prefix = ''): RawRow[] {
  if (
    typeof value !== 'object' ||
    value === null ||
    Array.isArray(value)
  ) {
    return [{ key: prefix, value: JSON.stringify(value) ?? 'null' }];
  }
  const rows: RawRow[] = [];
  for (const [key, child] of Object.entries(value)) {
    rows.push(...flatten(child, prefix === '' ? key : `${prefix}.${key}`));
  }
  return rows;
}
