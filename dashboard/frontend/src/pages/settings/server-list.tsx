import type { UpstreamServerConfig } from '../../api/types';
import { Icon } from '../../shell/icon';
import { MAX_UPSTREAM_SERVERS, UPSTREAM_PROTOCOLS } from './metadata';

/**
 * `[[dns.upstreams.servers]]` as a bounded row editor.
 *
 * **The array is one patch key.** `config_store.rs::merge` replaces arrays
 * rather than merging them — a half-merged array of tables has no meaning — so
 * editing one cell sends the whole list, and this component's `onChange` always
 * hands back every row.
 *
 * Order is meaning, not presentation: the index is what a query reports as its
 * answering endpoint (CONTEXT.md §Answering Endpoint), so the rows carry their
 * index and moving one changes what `endpoint 1` refers to.
 */
export function UpstreamServers({
  rows,
  onChange,
}: {
  rows: readonly UpstreamServerConfig[];
  onChange: (next: readonly UpstreamServerConfig[]) => void;
}) {
  const replace = (index: number, patch: Partial<UpstreamServerConfig>) => {
    onChange(
      rows.map((row, at) => (at === index ? { ...row, ...patch } : row)),
    );
  };

  return (
    <div class="upstream-rows">
      {/* Keyed by position, and only by position. The list is positional — the
          index *is* the answering endpoint — and a key carrying `row.address`
          is a key the address field's own keystrokes change: the row is
          remounted on every character and the caret goes with it. */}
      {rows.map((row, index) => (
        <div class="upstream-row" key={index}>
          <span class="ep-index mono" aria-hidden="true">
            {index}
          </span>
          <input
            class="field-input mono"
            type="text"
            spellcheck={false}
            autocapitalize="off"
            aria-label={`Upstream ${index + 1} address`}
            value={row.address}
            onInput={(event) =>
              replace(index, {
                address: (event.target as HTMLInputElement).value,
              })
            }
          />
          <select
            class="field-input"
            aria-label={`Upstream ${index + 1} protocol`}
            value={row.protocol}
            onChange={(event) =>
              replace(index, {
                protocol: (event.target as HTMLSelectElement).value,
              })
            }
          >
            {UPSTREAM_PROTOCOLS.map((protocol) => (
              <option key={protocol} value={protocol}>
                {protocol}
              </option>
            ))}
          </select>
          <input
            class="field-input mono"
            type="text"
            spellcheck={false}
            autocapitalize="off"
            placeholder="hostname"
            aria-label={`Upstream ${index + 1} hostname`}
            value={row.hostname ?? ''}
            onInput={(event) => {
              const text = (event.target as HTMLInputElement).value;
              // `null` rather than `""`: the schema's `Option<String>` means
              // "not set", and an empty string is a hostname of length zero.
              replace(index, { hostname: text.trim() === '' ? null : text });
            }}
          />
          <button
            type="button"
            class="iconbtn"
            title="Remove this endpoint"
            aria-label={`Remove upstream ${index + 1}`}
            disabled={rows.length <= 1}
            onClick={() => onChange(rows.filter((_, at) => at !== index))}
          >
            <Icon name="trash" size={15} />
          </button>
        </div>
      ))}
      <div class="upstream-rows-foot">
        <button
          type="button"
          class="btn g"
          disabled={rows.length >= MAX_UPSTREAM_SERVERS}
          onClick={() =>
            onChange([...rows, { address: '', protocol: 'udp', hostname: null }])
          }
        >
          Add endpoint
        </button>
        <span class="note">
          {rows.length} of {MAX_UPSTREAM_SERVERS} — at least one is required, and
          a longer list is rejected at load rather than truncated.
        </span>
      </div>
    </div>
  );
}
