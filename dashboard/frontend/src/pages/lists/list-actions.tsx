import type { ListItem } from '../../api/types';

/**
 * The row's actions. A `rejected` row gets a different primary action, not a
 * disabled one: the content gate refuses the body **against the cached
 * baseline**, and neither a refresh nor a disable/enable clears that copy — so
 * a source that legitimately restructured stays rejected on every attempt,
 * across restarts. The API's recovery contract is `DELETE` then re-add, and
 * this is the page an operator opens when a list is broken, so the way out
 * belongs on it.
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
  return (
    <div class="row-actions">
      {item.last_status === 'rejected' ? (
        <button type="button" class="linky" onClick={onReadd}>
          Delete and re-add
        </button>
      ) : (
        <button
          type="button"
          class="linky"
          disabled={pending || !item.enabled}
          onClick={onRefresh}
        >
          {pending ? 'refresh requested' : 'Refresh'}
        </button>
      )}
      <button type="button" class="linky" onClick={onEdit}>
        Edit
      </button>
      <button type="button" class="linky danger" onClick={onRemove}>
        Remove
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
