import { useCallback, useEffect, useRef, useState } from 'preact/hooks';
import {
  addList,
  deleteList,
  patchList,
  refreshList,
} from '../api/lists';
import { getTelemetry } from '../api/telemetry';
import type { ListItem, ListsResponse } from '../api/types';
import { Card } from '../components/card';
import { ConfirmDialog } from '../components/confirm-dialog';
import { EmptyState } from '../components/empty-state';
import { Figure } from '../components/figure';
import { ErrorState } from '../components/error-state';
import { useRefresh } from '../refresh/use-refresh';
import type { PageProps } from '../router/routes';
import { refresh, socket } from '../services';
import { nowMs } from '../lifecycle/timers';

import { AddListDialog } from './lists/add-list-dialog';
import { EditIntervalDialog } from './lists/edit-interval-dialog';
import { ListActions, ListToggle } from './lists/list-actions';
import { ListRow } from './lists/list-row';
import { RefreshAllDialog } from './lists/refresh-all-dialog';

/**
 * The page an operator opens when a list is broken, so the way out is on it.
 *
 * **Two read paths, deliberately.** The inventory goes through the shared
 * refresh registry — the route declares `endpoints: ['lists']`, the shell
 * acquires it, and the registry owns the only timer. `/telemetry` is read as a
 * plain **one-shot on mount** for one field, the last compile duration: it does
 * not drift on its own, and every compile on this page is one the page itself
 * caused. Putting a standing timer on `/telemetry` to render one figure is what
 * the route-scoped invariant rules out; reading it once is not.
 *
 * **No `RefreshCluster` anywhere on this page**, for either endpoint. The
 * artboard draws *mutation* controls — Refresh all, Add list — and a mutation
 * is not a refresh control. The `lists` interval is a browser-wide preference
 * set from the Dashboard's Ruleset card, and every event that actually changes
 * the inventory invalidates it immediately.
 */
export function Lists(_props: PageProps) {
  const inventory = useRefresh<ListsResponse>(refresh, 'lists');
  const [compileSeconds, setCompileSeconds] = useState<number | null>(null);
  const [now, setNow] = useState(nowMs);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  /** Ids whose `202` has been accepted and whose `list_refreshed` has not
   *  arrived. Bounded by the number of configured lists. */
  const [pending, setPending] = useState<readonly string[]>([]);
  const [mutationError, setMutationError] = useState<Error | null>(null);

  // One in-flight `/telemetry` read at a time. `POST /lists/refresh` emits one
  // `list_refreshed` per list, so fifteen lists means fifteen events inside one
  // second — this flag is what turns them into one request, the same job
  // `registry.invalidate` does for the inventory.
  const inFlight = useRef(false);
  const controller = useRef<AbortController | null>(null);

  const readCompileDuration = useCallback(() => {
    if (inFlight.current) return;
    inFlight.current = true;
    const next = new AbortController();
    controller.current = next;
    getTelemetry(next.signal)
      .then((telemetry) => {
        setCompileSeconds(telemetry.ruleset.compile_duration_seconds);
      })
      .catch(() => {
        // One figure in a header card. A failure leaves it unread rather than
        // failing the page.
      })
      .finally(() => {
        inFlight.current = false;
      });
  }, []);

  useEffect(() => {
    readCompileDuration();
    return () => controller.current?.abort();
  }, [readCompileDuration]);

  /**
   * The event carries no reason — it is a nudge to re-read `GET /lists`, where
   * `last_error` has it. `invalidate` is the same call the manual Refresh makes
   * and it already coalesces concurrent requests into one, so a fifteen-list
   * batch costs one inventory read and one `/telemetry` read.
   */
  useEffect(
    () =>
      socket.on('list_refreshed', (event) => {
        void refresh.invalidate('lists');
        readCompileDuration();
        setNow(nowMs());
        const id = event['id'];
        // The pending row is cleared by its own event, which is the only thing
        // that actually knows the refresh finished.
        if (typeof id === 'string') {
          setPending((current) => current.filter((entry) => entry !== id));
        }
      }),
    [readCompileDuration],
  );

  const data = inventory.data;
  const items = data?.items ?? [];
  const enabled = items.filter((item) => item.enabled).length;

  /** Every mutation ends the same way: revalidate the inventory through the
   *  registry — the same call the manual Refresh makes, coalescing included. */
  const revalidate = useCallback(() => {
    void refresh.invalidate('lists');
    readCompileDuration();
  }, [readCompileDuration]);

  const run = useCallback(
    (id: string, work: Promise<unknown>) => {
      setBusyId(id);
      setMutationError(null);
      work
        .then(revalidate)
        .catch((cause: unknown) => {
          setMutationError(
            cause instanceof Error ? cause : new Error(String(cause)),
          );
        })
        .finally(() => setBusyId(null));
    },
    [revalidate],
  );

  /**
   * `202 Accepted`. The row enters a pending state cleared by the
   * `list_refreshed` event for that id, **not** by the response — the response
   * says the request was taken, and the outcome arrives later.
   */
  const refreshOne = useCallback(
    (id: string) => {
      setPending((current) =>
        current.includes(id) ? current : [...current, id],
      );
      refreshList(id).catch((cause: unknown) => {
        setPending((current) => current.filter((entry) => entry !== id));
        setMutationError(
          cause instanceof Error ? cause : new Error(String(cause)),
        );
      });
    },
    [],
  );

  /**
   * The `rejected` recovery, and the reason it is a sequence rather than one
   * call: `DELETE` removes the cached copy that *is* the content gate's
   * baseline, so the re-add's first fetch has none to collapse against. If the
   * `DELETE` answers `500` the list is still there and the sequence stops —
   * the re-add is never attempted against a baseline that was not removed.
   */
  const deleteAndReadd = useCallback(
    (item: ListItem) => {
      setBusyId(item.id);
      setMutationError(null);
      deleteList(item.id)
        .then(() =>
          addList({
            id: item.id,
            url: item.url,
            enabled: item.enabled,
            refresh_hours: item.refresh_hours,
          }),
        )
        .then(revalidate)
        .catch((cause: unknown) => {
          setMutationError(
            cause instanceof Error ? cause : new Error(String(cause)),
          );
          revalidate();
        })
        .finally(() => setBusyId(null));
    },
    [revalidate],
  );

  return (
    <>
      {/* The artboard draws these two in the page header, which the shell owns
          and renders as a title alone. They sit at the top of the page body
          instead — the same place on screen, without a shell seam this task
          does not need. */}
      <div class="page-actions">
        <p class="sub page-sub">
          Adding, enabling or removing a list rewrites{' '}
          <span class="mono">[[rules.lists]]</span> in the config before the
          change reaches the engine — the file and the running engine never
          disagree.
        </p>
        <div class="page-buttons">
          <button
            type="button"
            class="btn g"
            onClick={() => setDialog({ kind: 'refresh-all' })}
          >
            Refresh all
          </button>
          <button
            type="button"
            class="btn"
            onClick={() => setDialog({ kind: 'add' })}
          >
            Add list
          </button>
        </div>
      </div>

      {mutationError !== null && <ErrorState error={mutationError} />}

      <Card bodyClass="figure-row">
        <Figure
          value={
            data === null ? '—' : data.compiled_rules.toLocaleString()
          }
          label="compiled rules"
        />
        <Figure
          value={
            data === null ? '—' : data.duplicates_removed.toLocaleString()
          }
          label="duplicates removed"
        />
        <Figure
          value={
            compileSeconds === null ? '—' : `${compileSeconds.toFixed(2)} s`
          }
          label="last compile"
        />
        <Figure
          value={
            data === null
              ? '—'
              : `${String(enabled)} / ${String(items.length)}`
          }
          label="enabled / configured"
        />
      </Card>

      <Card
        title="Configured lists"
        secondary={
          <span class="list-legend">
            <span class="sw" style={{ background: 'var(--tier-dns)' }} />
            domain tier
            <span class="sw" style={{ background: 'var(--tier-url)' }} />
            URL tier
            <span class="sw" style={{ background: 'var(--tier-inactive)' }} />
            not answered by any tier yet
          </span>
        }
        bodyClass="lists-body"
      >
        {inventory.error !== null && data === null ? (
          <ErrorState error={inventory.error} />
        ) : items.length === 0 ? (
          <EmptyState title="No lists configured">
            Nothing is being fetched. Add a list by URL or point one at a
            mounted file.
          </EmptyState>
        ) : (
          <div class="lists-table">
            <div class="list-head" aria-hidden="true">
              <span class="l-id">List</span>
              <span class="l-meta">
                <span class="l-on">On</span>
                <span class="l-every">Every</span>
                {/* Two lines each, so the two refresh columns are as narrow as
                    the values they carry rather than as wide as their own
                    labels. */}
                <span class="l-last">
                  <div>Last</div>
                  <div>Refresh</div>
                </span>
                <span class="l-next">
                  <div>Next</div>
                  <div>Refresh</div>
                </span>
              </span>
              <span class="l-status">Status</span>
              <span class="l-rules">Rules</span>
              <span class="l-total">Total</span>
              <span class="l-actions" />
            </div>
            {items.map((item) => (
              <ListRow
                key={item.id}
                item={item}
                now={now}
                toggle={
                  <ListToggle
                    item={item}
                    busy={busyId === item.id}
                    onToggle={(next) =>
                      run(item.id, patchList(item.id, { enabled: next }))
                    }
                  />
                }
                actions={
                  <ListActions
                    item={item}
                    pending={pending.includes(item.id)}
                    onRefresh={() => refreshOne(item.id)}
                    onEdit={() => setDialog({ kind: 'edit', item })}
                    onRemove={() => setDialog({ kind: 'remove', item })}
                    onReadd={() => setDialog({ kind: 'readd', item })}
                  />
                }
              />
            ))}
          </div>
        )}
        <p class="note lists-footnote">
          The three counts partition the list: a rule answers a domain question,
          answers an HTTP request, or no tier answers it yet (cosmetic,{' '}
          <span class="mono">$client</span>, unsupported). Inactive is not
          broken.
          <span class="lists-footnote-second">
            Refresh all runs one pass and recompiles once, not once per list. A
            list that fails is reported and skipped; the rest still refresh.
          </span>
        </p>
      </Card>

      {dialog?.kind === 'add' && (
        <AddListDialog onClose={() => setDialog(null)} onAdded={revalidate} />
      )}
      {dialog?.kind === 'refresh-all' && (
        <RefreshAllDialog
          onClose={() => setDialog(null)}
          onDone={revalidate}
        />
      )}
      {dialog?.kind === 'edit' && (
        <EditIntervalDialog
          item={dialog.item}
          onClose={() => setDialog(null)}
          onSaved={revalidate}
        />
      )}
      {dialog?.kind === 'remove' && (
        <ConfirmDialog
          title={`Remove ${dialog.item.id}?`}
          confirmLabel="Remove"
          onCancel={() => setDialog(null)}
          onConfirm={() => {
            run(dialog.item.id, deleteList(dialog.item.id));
            setDialog(null);
          }}
        >
          The list is dropped from <span class="mono">[[rules.lists]]</span> and
          its cached copy is deleted, so its{' '}
          {dialog.item.rules_total.toLocaleString()} rules stop serving at the
          next compile.
        </ConfirmDialog>
      )}
      {dialog?.kind === 'readd' && (
        <ConfirmDialog
          title={`Delete and re-add ${dialog.item.id}?`}
          confirmLabel="Delete and re-add"
          onCancel={() => setDialog(null)}
          onConfirm={() => {
            deleteAndReadd(dialog.item);
            setDialog(null);
          }}
        >
          A rejected body was refused against the <em>cached copy</em>, and
          neither a refresh nor a disable/enable clears that copy — so a source
          that legitimately restructured stays rejected on every attempt. This
          deletes the list and immediately re-adds it with the same id, source
          and interval, leaving its first fetch with no baseline to collapse
          against. If the delete fails the list is kept and nothing else runs.
        </ConfirmDialog>
      )}
    </>
  );
}

type Dialog =
  | null
  | { kind: 'add' }
  | { kind: 'refresh-all' }
  | { kind: 'edit'; item: ListItem }
  | { kind: 'remove'; item: ListItem }
  | { kind: 'readd'; item: ListItem };

export default Lists;
