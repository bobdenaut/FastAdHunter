import type { ListItem } from '../../api/types';
import { StatusPill } from '../../components/status-pill';

/**
 * The five values of `last_status`, plus the artboard's `DISABLED` pill — which
 * is not a status at all but the state of `enabled`.
 *
 * **A disabled list still shows what its last attempt did.** `DISABLED` alone
 * would make a list that failed look clean, so the real `last_status` follows
 * it as secondary text whenever there is one.
 *
 * `last_status` describes the last refresh *attempt*; the `rules_*` counts
 * beside it describe the copy currently **serving**. `failed` with a non-zero
 * `rules_total` is the normal report for a list whose download broke and whose
 * rules keep blocking, which is why every failure says so out loud.
 */
export function ListStatus({ item }: { item: ListItem }) {
  const status = item.last_status;

  return (
    <>
      {item.enabled ? (
        <StatusPill status={status} />
      ) : (
        <>
          <StatusPill status="disabled" />
          {status !== 'never' && (
            <div class="note list-status-secondary">last attempt: {status}</div>
          )}
        </>
      )}
      {item.enabled && <Body item={item} />}
    </>
  );
}

function Body({ item }: { item: ListItem }) {
  switch (item.last_status) {
    case 'ok':
      return null;
    case 'degraded':
      // Not a milder `ok`: the fetch succeeded and most of the body failed to
      // parse, which is the signature of a format misdetection. The list is
      // almost certainly contributing far fewer rules than it should.
      return (
        <>
          <div class="note list-status-alert">
            fetch succeeded, most of the body failed to parse — likely a format
            misdetection
          </div>
          <div class="note">
            check its syntax against RULE_ENGINE.md §Supported formats
          </div>
        </>
      );
    case 'failed':
      return (
        <>
          {item.last_error !== undefined && (
            <div class="note list-status-alert">{item.last_error}</div>
          )}
          <div class="note">last good copy still serving</div>
        </>
      );
    case 'rejected':
      return (
        <>
          {item.last_error !== undefined && (
            <div class="note list-status-alert">{item.last_error}</div>
          )}
          <div class="note">
            content gate refused the body before it could commit
          </div>
        </>
      );
    case 'never':
      return <div class="note">not fetched yet</div>;
  }
}
