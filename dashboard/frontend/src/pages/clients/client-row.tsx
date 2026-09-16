import type { ComponentChildren } from 'preact';
import type { Client } from '../../api/types';
import { PolicyChip } from '../../components/policy-chip';
import { blockedPercent } from '../../derive';
import type { Classification } from '../../policy/assignment';
import { Icon } from '../../shell/icon';
import { lastSeenLabel } from '../../time';

/**
 * One client, one DOM, both widths. Desktop lays the cells as
 * `Clients.dc.html`'s eight columns; below 768 px the same cells become
 * `MobileClients.dc.html`'s card. The row is a `subgrid` of the table's own
 * tracks, so the header and every row share **one** grid definition rather than
 * two that agree until somebody edits one of them.
 *
 * The action is a 44 × 44 glyph, never a label: a text action cell is measured
 * per row, and a row whose action set changes with its state then shifts every
 * other cell sideways — p5-06's F1, closed here by construction.
 *
 * T9 — the share bar's width **is** the percentage, not a normalisation
 * against the table. That is `Clients.dc.html`'s own arithmetic and it answers
 * a different question from the Dashboard's top-N bars, so the card's legend
 * says which.
 */
export function ClientRow({
  client,
  classification,
  now,
  open,
  busy,
  onToggle,
  children,
}: {
  client: Client;
  classification: Classification;
  now: number;
  open: boolean;
  busy: boolean;
  onToggle: () => void;
  /** The expanded region: the two actions, or the rename field. */
  children?: ComponentChildren;
}) {
  const share = blockedPercent(client.queries_24h, client.blocked_24h);

  return (
    <div class={open ? 'client-row is-open' : 'client-row'}>
      <div class="c-ip mono">{client.ip}</div>
      <div class={client.name === null ? 'c-name unnamed' : 'c-name mono'}>
        {client.name ?? 'unnamed'}
      </div>
      <div class="c-policy">
        <PolicyChip classification={classification} />
      </div>
      <div class="c-queries mono">
        <span class="c-label">queries</span>
        {client.queries_24h.toLocaleString()}
      </div>
      <div class="c-blocked mono">
        <span class="c-label">blocked</span>
        {client.blocked_24h.toLocaleString()}
      </div>
      <div class="c-share">
        <div class="bar">
          <span
            class="c-share-fill"
            style={{ width: `${String(Math.min(100, share))}%` }}
          />
        </div>
        <div class="note num mono c-share-figure">
          {client.queries_24h === 0 ? '0 %' : `${share.toFixed(1)} %`}
        </div>
      </div>
      <div class="c-seen note mono">{lastSeenLabel(client.last_seen, now)}</div>
      <div class="c-actions">
        <button
          type="button"
          class="iconbtn"
          disabled={busy}
          aria-expanded={open}
          aria-label={`Edit ${client.ip}`}
          onClick={onToggle}
        >
          <Icon name="edit" size={16} />
        </button>
      </div>
      {open && <div class="c-expand">{children}</div>}
    </div>
  );
}
