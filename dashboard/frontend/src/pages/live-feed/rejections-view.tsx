import { useEffect, useMemo, useState } from 'preact/hooks';
import { ApiError } from '../../api/core';
import {
  documentErrorDetails,
  getInterception,
  putInterception,
} from '../../api/interception';
import type { InterceptionDocument, QueryEvent } from '../../api/types';
import { ConfirmDialog } from '../../components/confirm-dialog';
import { EmptyState } from '../../components/empty-state';
import { ErrorState } from '../../components/error-state';
import { blockNavigation } from '../../router/router';
import { eventClock } from '../../time';
import {
  groupRejections,
  isExcluded,
  withExclusion,
  type RejectionGroup,
} from './rejections';

/**
 * The hosts whose clients refused the leaf we minted for them, grouped by
 * client and host, each with one action: exclude the exact host observed.
 *
 * **Detection never mutates policy** (ADR-0008 §detect ≠ auto-exclude). Nothing
 * here writes without a click and an explicit confirmation naming the host, and
 * the only write it can perform is appending that one host — there is no
 * widening control, because widening correctly needs public-suffix semantics
 * and a label walk would put "exclude an entire TLD" one pixel from the safe
 * button.
 *
 * **Bounded and timerless.** The groups are derived from the ring snapshot the
 * page already holds, so rows older than the ring are gone and the empty state
 * says so. The document is read once on entry and again only from this view's
 * own successful `PUT`: nothing polls, and `/live-feed` still declares no
 * endpoint.
 */
export function RejectionsView({
  rows,
  narrow,
}: {
  rows: readonly QueryEvent[];
  narrow: boolean;
}) {
  const [document, setDocument] = useState<InterceptionDocument | null>(null);
  const [loadError, setLoadError] = useState<Error | null>(null);
  /** The group awaiting confirmation, by key — never the row index, which the
   *  next flush can renumber under the open dialog. */
  const [confirming, setConfirming] = useState<string | null>(null);
  const [saving, setSaving] = useState<string | null>(null);
  /** The server's answer to the last exclude, against the group that asked for
   *  it. One at a time: a second attempt replaces the first's message. */
  const [notice, setNotice] = useState<{ key: string; text: string } | null>(
    null,
  );
  const [saveError, setSaveError] = useState<Error | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    getInterception(controller.signal)
      .then((fresh) => {
        setDocument(fresh);
        setLoadError(null);
      })
      .catch((cause: unknown) => {
        if (cause instanceof Error && cause.name === 'AbortError') return;
        setLoadError(cause instanceof Error ? cause : new Error(String(cause)));
      });
    return () => controller.abort();
  }, []);

  // Memoised on the snapshot, as `visible` is: the walk is O(rows) over at most
  // 500 held rows and runs on every flush the feed makes while this view is up.
  const groups = useMemo(() => groupRejections(rows), [rows]);

  const pending = groups.find((group) => keyOf(group) === confirming) ?? null;

  /**
   * Read, append, write the whole document back — the sequence ADR-0008 fixes.
   *
   * The read is fresh rather than the mounted copy: another tab may have edited
   * the document since this view opened, and last-write-wins over a stale copy
   * would silently drop its entries. The response is authoritative and replaces
   * the local copy; a rejection leaves it exactly as it was, because a rejected
   * `PUT` changed nothing on the server either.
   */
  const exclude = (host: string, key: string) => {
    setConfirming(null);
    setSaving(key);
    setNotice(null);
    setSaveError(null);
    const unblock = blockNavigation();
    getInterception()
      .then((current) => putInterception(withExclusion(current, host)))
      .then((stored) => {
        setDocument(stored);
      })
      .catch((cause: unknown) => {
        const text = excludeMessage(cause);
        if (text === null) {
          setSaveError(cause instanceof Error ? cause : new Error(String(cause)));
          return;
        }
        setNotice({ key, text });
      })
      .finally(() => {
        unblock();
        setSaving(null);
      });
  };

  if (loadError !== null) return <ErrorState error={loadError} />;

  if (groups.length === 0) {
    return (
      <EmptyState title="No client has rejected our certificate since this page opened">
        A row appears here when an intercepted client answers our minted leaf
        with a rejecting alert. Nothing is retained between visits — this tab
        holds the whole of it, and the count is what a retrying application
        leaves behind.
      </EmptyState>
    );
  }

  const cells = (group: RejectionGroup) => {
    const key = keyOf(group);
    const excluded =
      document !== null && isExcluded(group.host, document.exclude_domains);
    return {
      key,
      excluded,
      busy: saving === key,
      message: notice?.key === key ? notice.text : null,
      action: () => setConfirming(key),
    };
  };

  return (
    <>
      {narrow ? null : (
        <div class="feed-scroll">
          <table class="t">
            <thead>
              <tr>
                <th>Last seen</th>
                <th>Client</th>
                <th>Host</th>
                <th class="num">Count</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody>
              {groups.map((group) => {
                const cell = cells(group);
                return (
                  <tr key={cell.key}>
                    <td class="mono">{eventClock(group.last)}</td>
                    <td>{group.clientName ?? group.client}</td>
                    <td class="mono">{group.host}</td>
                    <td class="num">{group.count}</td>
                    <td>
                      <Action
                        host={group.host}
                        excluded={cell.excluded}
                        busy={cell.busy}
                        ready={document !== null}
                        message={cell.message}
                        onExclude={cell.action}
                      />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {!narrow ? null : (
        <div class="feed-cards">
          {groups.map((group) => {
            const cell = cells(group);
            return (
              <article class="ev" key={cell.key}>
                <div class="ev-top">
                  <span class="feed-kind">https</span>
                  <span class="note mono">{eventClock(group.last)}</span>
                </div>
                <div class="mono ev-domain">{group.host}</div>
                <div class="ev-meta note">
                  <span>{group.clientName ?? group.client}</span>
                  <span class="mono">
                    {group.count} {group.count === 1 ? 'rejection' : 'rejections'}
                  </span>
                </div>
                <div class="ev-meta note">
                  <Action
                    host={group.host}
                    excluded={cell.excluded}
                    busy={cell.busy}
                    ready={document !== null}
                    message={cell.message}
                    onExclude={cell.action}
                  />
                </div>
              </article>
            );
          })}
        </div>
      )}

      {saveError !== null && <ErrorState error={saveError} />}

      <p class="note">
        A row here means the client refused the certificate we present for that
        host, not that anything was blocked — the connection was closed and
        nothing was excluded. Excluding a host splices it for every intercepted
        client from the next connection on; an already-open session finishes
        under the lists it was admitted with.
        <span class="footnote-line">
          Only the exact host observed can be excluded from here. Covering
          everything beneath a parent name is a deliberate act, performed in the
          Interception card on the Settings page.
        </span>
      </p>

      {pending !== null && (
        <ConfirmDialog
          title={`Exclude ${pending.host} from interception?`}
          confirmLabel="Exclude this host"
          onCancel={() => setConfirming(null)}
          onConfirm={() => exclude(pending.host, keyOf(pending))}
        >
          <>
            <p class="note">
              <span class="mono">{pending.host}</span> stops being intercepted
              for <b>every</b> listed client, from its next connection. Its
              parent name is not excluded — widening is done by editing the
              document in Settings.
            </p>
            <p class="note">
              Nothing else in the document changes, and nothing is sent until
              you confirm.
            </p>
          </>
        </ConfirmDialog>
      )}
    </>
  );
}

/** Client and host, which is what a group *is*. Stable across flushes, unlike
 *  the position of a row whose neighbours keep arriving. */
function keyOf(group: { client: string; host: string }): string {
  return `${group.client} ${group.host}`;
}

/**
 * The one control, or the marker that says it is not needed.
 *
 * A host already covered by the document — itself or under a parent entry —
 * shows as excluded rather than offering a button whose only outcome would be a
 * `duplicate` rejection or a redundant entry.
 */
function Action({
  host,
  excluded,
  busy,
  ready,
  message,
  onExclude,
}: {
  host: string;
  excluded: boolean;
  busy: boolean;
  ready: boolean;
  message: string | null;
  onExclude: () => void;
}) {
  if (excluded) {
    return (
      <span class="pill neutral" title={`${host} is already excluded`}>
        excluded
      </span>
    );
  }
  return (
    <>
      <button
        type="button"
        class="btn g"
        disabled={busy || !ready}
        onClick={onExclude}
      >
        {busy ? 'Excluding…' : 'Exclude'}
      </button>
      {message !== null && (
        <span class="note" role="status">
          {message}
        </span>
      )}
    </>
  );
}

/**
 * What to print beside the row, or `null` for a failure that is not the
 * server's answer about this document — those go to `ErrorState`, which knows
 * how to draw a network failure.
 *
 * Only `details.reason` is branched on. `message` is the API's own words and is
 * shown verbatim where no better sentence exists; it is never parsed.
 */
function excludeMessage(cause: unknown): string | null {
  const details = documentErrorDetails(cause);
  if (details === null) {
    // A `503` or a `500` is still the server answering about this write, and
    // its `message` says which. Anything that never reached the server is not.
    return cause instanceof ApiError ? cause.message : null;
  }
  switch (details.reason) {
    case 'duplicate':
      return 'Already excluded — the document already covers this host.';
    case 'over_cap':
      return `Not saved — ${details.list} would hold ${String(details.len)} entries and the cap is ${String(details.cap)}. Remove one in Settings first.`;
    case 'invalid_entry':
      return `Not saved — the API rejected ${details.entry} as an entry in ${details.list}.`;
    case 'shape':
      return 'Not saved — the API did not accept the document as sent.';
  }
}
