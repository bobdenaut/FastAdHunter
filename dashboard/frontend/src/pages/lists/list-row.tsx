import type { ComponentChildren } from 'preact';
import type { ListItem } from '../../api/types';
import { StageBar } from '../../components/stage-bar';
import {
  lastRefreshLabel,
  nextRefreshLabel,
  type RefreshLabel,
} from '../../time';
import { ListStatus } from './list-status';

/**
 * One DOM at both widths, as everywhere else in this task: a grid whose cells
 * flow as the artboard's eight table columns on desktop and re-lay themselves
 * into `MobileLists.dc.html`'s card below 768 px. A seven-column horizontal
 * scroller is not an answer on the page an operator opens when a list is
 * broken.
 *
 * R15 — the partition bar is each of `rules_active_dns`, `rules_active_url` and
 * `rules_inactive` over `rules_total`. The three counts partition the total, so
 * the bar is the response's own arithmetic rather than a normalisation.
 *
 * `parse_errors` sits on the same mono line as the partition because it means
 * nothing alone: it reads `0` for a clean copy **and** for one contributing
 * nothing, so it is read beside `enabled` and `rules_total`.
 */
export function ListRow({
  item,
  now,
  actions,
  toggle,
}: {
  item: ListItem;
  now: number;
  actions?: ComponentChildren;
  toggle?: ComponentChildren;
}) {
  const alert =
    item.enabled &&
    (item.last_status === 'failed' ||
      item.last_status === 'rejected' ||
      item.last_status === 'degraded');

  return (
    <div class={alert ? 'list-row is-alert' : 'list-row'}>
      <div class="l-id">
        <div class="mono l-name">{item.id}</div>
        <div class="note mono l-source">{item.url}</div>
      </div>

      <div class="l-meta">
        <div class="l-on">{toggle}</div>
        <div class="l-every mono">
          {item.enabled ? `${String(item.refresh_hours)} h` : '—'}
        </div>
        <RefreshCell class="l-last" label={lastRefreshLabel(item.last_refresh, now)} />
        {/* Disabled lists are not scheduled at all, which the dash says and a
            stale due time would contradict. */}
        <RefreshCell
          class="l-next"
          label={
            item.enabled
              ? nextRefreshLabel(item.last_refresh, item.refresh_hours, now)
              : { day: '—', time: null }
          }
        />
      </div>

      <div class="l-status">
        <ListStatus item={item} />
      </div>

      <div class="l-rules">
        {item.enabled ? (
          <>
            <div class="l-rules-bar">
              <StageBar
                segments={[
                  {
                    label: 'dns',
                    value: item.rules_active_dns,
                    colour: 'var(--tier-dns)',
                  },
                  {
                    label: 'url',
                    value: item.rules_active_url,
                    colour: 'var(--tier-url)',
                  },
                  {
                    label: 'inactive',
                    value: item.rules_inactive,
                    colour: 'var(--tier-inactive)',
                  },
                ]}
              />
              <span class="mono l-total-inline">
                {item.rules_total.toLocaleString()} rules
              </span>
            </div>
            <div class="note mono l-partition">
              {item.rules_active_dns.toLocaleString()} dns ·{' '}
              {item.rules_active_url.toLocaleString()} url ·{' '}
              {item.rules_inactive.toLocaleString()} inactive
              <span class="l-parse">
                <span class="l-parse-sep"> · </span>
                {item.parse_errors.toLocaleString()} parse errors
              </span>
            </div>
          </>
        ) : (
          <>
            <div class="seg" />
            <div class="note mono l-partition">not compiled while disabled</div>
          </>
        )}
      </div>

      <div class="l-total mono">{item.rules_total.toLocaleString()}</div>
      <div class="l-actions">{actions}</div>
    </div>
  );
}

/**
 * The day over the clock, so a refresh time costs one narrow column instead of
 * one wide one. A label with no timestamp — `never`, `due`, `unscheduled` —
 * carries the whole answer on the day line and renders no second line at all,
 * rather than a blank one that would make the row taller for nothing.
 */
function RefreshCell({
  class: className,
  label,
}: {
  class: string;
  label: RefreshLabel;
}) {
  return (
    <div class={`${className} mono note`}>
      <div>{label.day}</div>
      {label.time !== null && <div>{label.time}</div>}
    </div>
  );
}
