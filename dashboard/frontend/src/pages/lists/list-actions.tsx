import type { ListItem } from '../../api/types';
import { Icon } from '../../shell/icon';

/**
 * The row's actions. A `rejected` row gets a different primary action, not a
 * disabled one: the content gate refuses the body **against the cached
 * baseline**, and neither a refresh nor a disable/enable clears that copy — so
 * a source that legitimately restructured stays rejected on every attempt,
 * across restarts. The API's recovery contract is `DELETE` then re-add, and
 * this is the page an operator opens when a list is broken, so the way out
 * belongs on it.
 *
 * **Glyphs, not words, and the reason is structural rather than decorative.**
 * As labels these three cost 200 px of a row that has eight columns, which put
 * an internal scrollbar on the table at every desktop width below 1247 px. They
 * also varied in width by state — `Refresh` 130 px, `refresh requested` 186 px,
 * `Delete and re-add` 189 px — which is what made the columns jump sideways
 * mid-action (F1). Three fixed 44 px targets are 132 px and cannot vary at all,
 * so the defect is gone by construction rather than by a magic track width.
 *
 * Each carries an `aria-label` and a `title`, so the word is one hover or one
 * screen reader away; the confirmations behind Remove and delete-and-re-add are
 * unchanged, and they are where the consequence is stated.
 */
export function ListActions({
  item,
  pending,
  onRefresh,
  onEdit,
  onRemove,
  onReadd,
}: {
  item: ListItem;
  pending: boolean;
  onRefresh: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onReadd: () => void;
}) {
  // `pending` is the 202's own state, cleared by the `list_refreshed` event
  // rather than by the response. As a glyph it is the same button in a busy
  // state — no second label, and therefore no second width.
  const refreshLabel = pending
    ? `Refresh requested for ${item.id}`
    : `Refresh ${item.id}`;
  return (
    <div class="row-actions">
      {item.last_status === 'rejected' ? (
        <button
          type="button"
          class="iconbtn"
          aria-label={`Delete and re-add ${item.id}`}
          title="Delete and re-add"
          onClick={onReadd}
        >
          <Icon name="restore" size={17} />
        </button>
      ) : (
        <button
          type="button"
          class={pending ? 'iconbtn is-busy' : 'iconbtn'}
          disabled={pending || !item.enabled}
          aria-label={refreshLabel}
          title={pending ? 'Refresh requested' : 'Refresh'}
          onClick={onRefresh}
        >
          <Icon name="refresh" size={17} />
        </button>
      )}
      <button
        type="button"
        class="iconbtn"
        aria-label={`Edit ${item.id}`}
        title="Edit"
        onClick={onEdit}
      >
        <Icon name="edit" size={17} />
      </button>
      <button
        type="button"
        class="iconbtn danger"
        aria-label={`Remove ${item.id}`}
        title="Remove"
        onClick={onRemove}
      >
        <Icon name="trash" size={17} />
      </button>
    </div>
  );
}

/** The artboard's pill switch, over a real checkbox so it keeps the keyboard
 *  and the accessibility tree. */
export function ListToggle({
  item,
  busy,
  onToggle,
}: {
  item: ListItem;
  busy: boolean;
  onToggle: (enabled: boolean) => void;
}) {
  return (
    <label class={item.enabled ? 'switch on' : 'switch'}>
      <input
        type="checkbox"
        checked={item.enabled}
        disabled={busy}
        aria-label={`${item.enabled ? 'Disable' : 'Enable'} ${item.id}`}
        onChange={(event) =>
          onToggle((event.target as HTMLInputElement).checked)
        }
      />
      <i />
    </label>
  );
}
