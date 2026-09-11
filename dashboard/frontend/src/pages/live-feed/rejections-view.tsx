import { useEffect, useMemo, useState } from 'preact/hooks';
import { getClients } from '../../api/clients';
import { ApiError } from '../../api/core';
import {
  documentErrorDetails,
  getInterception,
  putInterception,
} from '../../api/interception';
import type { Client, InterceptionDocument, QueryEvent } from '../../api/types';
import { ConfirmDialog } from '../../components/confirm-dialog';
import { EmptyState } from '../../components/empty-state';
import { ErrorState } from '../../components/error-state';
import { blockNavigation } from '../../router/router';
import { eventClock } from '../../time';
import {
  exclusionsOf,
  groupRejections,
  isExcluded,
  keyOf,
  summarizeClients,
  withExclusion,
  type ClientSummary,
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
  const [stored, setStored] = useState<InterceptionDocument | null>(null);
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
  /** Bumped by Retry: the mount read failed and the operator asked again. */
  const [attempt, setAttempt] = useState(0);
  /** The engine's per-client account (`GET /api/v1/clients`), read on mount
   *  and again on Retry, never polled — the dashboard does not poll, and the
   *  rows never depend on it: a failed read leaves the summary at "none". */
  const [clients, setClients] = useState<readonly Client[]>([]);

  useEffect(() => {
    const controller = new AbortController();
    getClients(controller.signal)
      .then((response) => setClients(response.items))
      .catch(() => undefined);
    return () => controller.abort();
  }, [attempt]);

  useEffect(() => {
    const controller = new AbortController();
    getInterception(controller.signal)
      .then((fresh) => {
        setStored(fresh);
        setLoadError(null);
      })
      .catch((cause: unknown) => {
        if (cause instanceof Error && cause.name === 'AbortError') return;
        setLoadError(cause instanceof Error ? cause : new Error(String(cause)));
      });
    return () => controller.abort();
  }, [attempt]);

  // Memoised on the snapshot, as `visible` is: the walk is O(rows) over at most
  // 500 held rows and runs on every flush the feed makes while this view is up.
  const groups = useMemo(() => groupRejections(rows), [rows]);
  const summaries = useMemo(
    () => summarizeClients(groups, clients),
    [groups, clients],
  );

  const pending =
    groups.find((group) => keyOf(group.client, group.host) === confirming) ??
    null;
  // One write at a time. A second read-append-`PUT` racing the first would
  // read the same base, and the later write would drop the earlier host — so
  // while one is in flight no row is offered, not only the one being saved.
  const ready = stored !== null && saving === null;

  /**
   * Read, append, write the whole document back — the sequence ADR-0008 fixes.
   *
   * The read is fresh rather than the mounted copy: another tab may have edited
   * the document since this view opened, and last-write-wins over a stale copy
   * would silently drop its entries. The fresh copy is adopted before the write
   * because it is also what a rejection describes — a `duplicate` says *this*
   * document already covers the host, and the row must read as excluded
   * against it. The response then replaces it; a rejected `PUT` changed nothing
   * on the server, so the fresh copy stands.
   */
  const exclude = (host: string, key: string) => {
    setConfirming(null);
    setSaving(key);
    setNotice(null);
    setSaveError(null);
    const unblock = blockNavigation();
    getInterception()
      .then((current) => {
        setStored(current);
        return putInterception(withExclusion(current, host));
      })
      .then((response) => {
        setStored(response);
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

  // Normalized once per document, not per group per flush.
  const exclusions = useMemo(
    () => (stored === null ? null : exclusionsOf(stored.exclude_domains)),
    [stored],
  );

  /**
   * The groups are the ring's and need no document; only the action does. So a
   * failed read is shown above them, with the buttons held back and a way to
   * ask again — not in place of them.
   */
  const failed =
    loadError === null ? null : (
      <>
        <ErrorState error={loadError} />
        <p class="note">
          The rows below are what the ring holds; excluding needs the document.{' '}
          <button
            type="button"
            class="btn g"
            onClick={() => setAttempt((count) => count + 1)}
          >
            Retry
          </button>
        </p>
      </>
    );

  if (groups.length === 0) {
    return (
      <>
        {failed}
        <EmptyState title="No client has rejected our certificate since this page opened">
          A row appears here when an intercepted client answers our minted leaf
          with a rejecting alert. Nothing is retained between visits — this tab
          holds the whole of it, and the count is what a retrying application
          leaves behind.
        </EmptyState>
      </>
    );
  }

  const cells = (group: RejectionGroup) => {
    const key = keyOf(group.client, group.host);
    const excluded = exclusions !== null && isExcluded(group.host, exclusions);
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
      {failed}
      <ClientsSummary clients={summaries} />

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
                        ready={ready}
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
                    ready={ready}
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
        under the lists it was admitted with. The lines above are the engine's
        per-client account, completed intercepted handshakes on record beside
        the rejections — a client that rejects and never completes is a reason
        to check the client, not to exclude the host.
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
          onConfirm={() => exclude(pending.host, keyOf(pending.client, pending.host))}
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

/**
 * One line per client in the view: rejections, hosts, and the engine's count
 * of completed intercepted handshakes on record. Stated as what was observed,
 * never as "the CA is installed": a completed handshake proves the client
 * accepted our leaf at that moment, and none on record is a reason to check
 * the client before excluding a host for everyone (p3-06-n3-alert-ab.md).
 */
function ClientsSummary({ clients }: { clients: readonly ClientSummary[] }) {
  if (clients.length === 0) return null;
  return (
    <div class="rejection-clients">
      {clients.map((client) => (
        <p class="note" key={client.client}>
          <b>{client.clientName ?? client.client}</b>{' '}
          <span class="mono">{client.rejections}</span>{' '}
          {client.rejections === 1 ? 'rejection' : 'rejections'} across{' '}
          <span class="mono">{client.hosts}</span>{' '}
          {client.hosts === 1 ? 'host' : 'hosts'}. Completed intercepted handshakes
          on record:{' '}
          {client.completed === null ? (
            <>
              <span class="mono">none</span>
              <span class="footnote-line">
                Nothing from this client has completed our handshake on record. Before excluding a host for every client, check this
                client's CA: one without it refuses every host, and an exclusion
                would not fix that. The CA is exported from{' '}
                <span class="mono">/api/v1/certificates</span>.
              </span>
            </>
          ) : (
            <>
              <span class="mono">{client.completed.count}</span>, last{' '}
              <span class="mono">{eventClock(client.completed.last)}</span>
            </>
          )}
        </p>
      ))}
    </div>
  );
}
