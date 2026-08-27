import { useCallback, useEffect, useRef, useState } from 'preact/hooks';
import { getLists } from '../api/lists';
import {
  createPolicy,
  deletePolicy,
  patchPolicy,
} from '../api/policies';
import { getPolicies } from '../api/policies';
import { getStats } from '../api/stats';
import type {
  ListsResponse,
  PoliciesResponse,
  Policy,
  PolicyStat,
  Stats,
} from '../api/types';
import { BusyModal } from '../components/busy-modal';
import { Card } from '../components/card';
import { ConfirmDialog } from '../components/confirm-dialog';
import { ErrorState } from '../components/error-state';
import { Figure } from '../components/figure';
import { blockNavigation } from '../router/router';
import type { PageProps } from '../router/routes';
import { ContentHeader } from '../shell/content-header';

import { AssignmentRows } from './policies/assignment-rows';
import { CostCard, CostFootnote } from './policies/cost-card';
import { DefaultCard } from './policies/default-card';
import { ListChips, PolicyTraffic } from './policies/policy-card';
import { PolicyDialog, type PolicyDraft } from './policies/policy-dialog';

/**
 * Named bundles of rule lists, assignable to clients and optionally scheduled.
 *
 * **Three entry one-shots and nothing else.** `/policies` is the page;
 * `/stats` is the only source of per-policy traffic, read once per mount for
 * one field because the alternative is a card the artboard draws with no data
 * behind it; `/lists` supplies the subset picker's ids and the rule count the
 * What-costs-what card states. No timer, no event, no re-read of `/stats` or
 * `/lists` after a mutation — a policy edit changes neither the list inventory
 * nor the 24 h traffic window.
 *
 * **The recompile boundary is the page's whole shape.** Creating a policy,
 * changing which lists it holds and deleting one all rebuild the ruleset
 * inline, so the request is held open for seconds; renaming, changing the
 * blocking-mode override and editing assignments return in milliseconds. The
 * first three confirm and then block in a modal that also blocks navigation —
 * they are never aborted, because a cancel between persist and swap leaves the
 * config ahead of the live matcher. The others do neither.
 */
export function Policies(_props: PageProps) {
  const [policies, setPolicies] = useState<PoliciesResponse | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [lists, setLists] = useState<ListsResponse | null>(null);
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [dialogError, setDialogError] = useState<Error | null>(null);
  const [mutationError, setMutationError] = useState<Error | null>(null);
  const [editing, setEditing] = useState<Policy | 'new' | null>(null);
  const [removing, setRemoving] = useState<Policy | null>(null);
  const [pending, setPending] = useState<PolicyDraft | null>(null);
  const [busy, setBusy] = useState<Busy | null>(null);
  const controller = useRef<AbortController | null>(null);

  const reloadPolicies = useCallback(() => {
    controller.current?.abort();
    const next = new AbortController();
    controller.current = next;
    return getPolicies(next.signal)
      .then((response) => {
        setPolicies(response);
        setLoadError(null);
      })
      .catch((cause: unknown) => {
        if (cause instanceof DOMException && cause.name === 'AbortError') return;
        setLoadError(cause instanceof Error ? cause : new Error(String(cause)));
      });
  }, []);

  useEffect(() => {
    const boot = new AbortController();
    controller.current = boot;
    Promise.all([
      getPolicies(boot.signal),
      getStats(boot.signal),
      getLists(boot.signal),
    ])
      .then(([policyList, statistics, inventory]) => {
        setPolicies(policyList);
        setStats(statistics);
        setLists(inventory);
        setLoadError(null);
      })
      .catch((cause: unknown) => {
        if (cause instanceof DOMException && cause.name === 'AbortError') return;
        setLoadError(cause instanceof Error ? cause : new Error(String(cause)));
      });
    return () => controller.current?.abort();
  }, []);

  /**
   * The one write path. `recompiles` decides everything visible about it: a
   * blocking modal, a navigation block and no abort, or none of the three.
   */
  const run = useCallback(
    (label: string, recompiles: boolean, work: () => Promise<unknown>) => {
      if (busy !== null) return;
      setBusy({ label, recompiles });
      setMutationError(null);
      setDialogError(null);
      const unblock = recompiles ? blockNavigation() : () => undefined;
      work()
        .then(() => {
          setEditing(null);
          setRemoving(null);
          return reloadPolicies();
        })
        .catch((cause: unknown) => {
          const error =
            cause instanceof Error ? cause : new Error(String(cause));
          // A dialog is still open for a create or an edit, so the message
          // belongs beside the field that caused it rather than behind it.
          if (editing !== null) setDialogError(error);
          else setMutationError(error);
        })
        .finally(() => {
          unblock();
          setBusy(null);
        });
    },
    [busy, editing, reloadPolicies],
  );

  const submit = useCallback(
    (draft: PolicyDraft) => {
      if (draft.recompiles) {
        setPending(draft);
        return;
      }
      const target = editing;
      if (target === null || target === 'new' || draft.patch === null) return;
      const patch = draft.patch;
      if (Object.keys(patch).length === 0) {
        setEditing(null);
        return;
      }
      run(target.id, false, () => patchPolicy(target.id, patch));
    },
    [editing, run],
  );

  const confirmPending = useCallback(() => {
    const draft = pending;
    setPending(null);
    if (draft === null) return;
    if (draft.create !== null) {
      const body = draft.create;
      run(body.id, true, () => createPolicy(body));
      return;
    }
    const target = editing;
    if (target === null || target === 'new' || draft.patch === null) return;
    const patch = draft.patch;
    run(target.id, true, () => patchPolicy(target.id, patch));
  }, [editing, pending, run]);

  const items = policies?.items ?? [];
  const configured = items.length;
  const total = configured + 1;
  const slotsLeft = CEILING - total;
  const assignmentsConfigured = items.reduce(
    (sum, policy) => sum + policy.assignments.length,
    0,
  );
  const timezone = policies?.timezone ?? null;

  const statFor = (id: string): PolicyStat | null =>
    stats?.policies.find((entry) => entry.policy === id) ?? null;

  return (
    <>
      <ContentHeader
        title="Policies"
        context={
          <>
            Named bundles of rule lists, assignable to clients and optionally
            scheduled — <span class="mono">GET /api/v1/policies</span>
          </>
        }
        actions={
          <button
            type="button"
            class="btn"
            disabled={policies === null || slotsLeft <= 0 || busy !== null}
            title={
              slotsLeft <= 0
                ? `the ceiling is ${String(CEILING)} policies, the implicit default included`
                : undefined
            }
            onClick={() => setEditing('new')}
          >
            New policy
          </button>
        }
      />

      <main class="wrap">
        {mutationError !== null && <ErrorState error={mutationError} />}
        {/* A failed re-read after a mutation: the cards below are stale and
            silence would hide it. */}
        {loadError !== null && policies !== null && (
          <ErrorState error={loadError} />
        )}
        {loadError !== null && policies === null ? (
          <ErrorState error={loadError} />
        ) : (
          <>
            <Card bodyClass="figure-row">
              <Figure
                value={
                  policies === null
                    ? '—'
                    : `${String(total)} / ${String(CEILING)}`
                }
                label="policies · ceiling"
              />
              <Figure
                value={
                  policies === null
                    ? '—'
                    : policies.active_assignments.toLocaleString()
                }
                label="assignments in force right now"
              />
              <Figure
                value={
                  policies === null ? '—' : assignmentsConfigured.toLocaleString()
                }
                label="assignments configured"
              />
              <div title={timezone ?? undefined}>
                <div class="figure mono figure-small">
                  {timezone === null ? '—' : timezone.split(',')[0]}
                </div>
                <div class="note">schedule timezone</div>
              </div>
            </Card>

            <div class="row c2 policy-grid">
              <DefaultCard stat={statFor(DEFAULT_ID)} />

              {items.map((policy) => (
                <Card
                  key={policy.id}
                  title={
                    <>
                      {policy.name}{' '}
                      <span class="note mono policy-id">{policy.id}</span>
                    </>
                  }
                  tools={
                    <span class="policy-tools">
                      <button
                        type="button"
                        class="btn g"
                        disabled={busy !== null}
                        onClick={() => setEditing(policy)}
                      >
                        Edit
                      </button>
                      <button
                        type="button"
                        class="btn g danger-btn"
                        disabled={busy !== null}
                        onClick={() => setRemoving(policy)}
                      >
                        Delete
                      </button>
                    </span>
                  }
                  className="policy-card"
                >
                  <ListChips lists={policy.lists} />
                  {policy.blocking_mode !== null && (
                    <p class="note policy-note">
                      blocking mode{' '}
                      <span class="mono">{policy.blocking_mode}</span>, overriding
                      the global setting
                    </p>
                  )}
                  <AssignmentRows assignments={policy.assignments} />
                  <PolicyTraffic stat={statFor(policy.id)} />
                </Card>
              ))}

              <section class="card policy-slot">
                <div>
                  <p class="note">
                    {slotsLeft} policy slot{slotsLeft === 1 ? '' : 's'} left
                  </p>
                  <button
                    type="button"
                    class="btn g"
                    disabled={slotsLeft <= 0 || busy !== null}
                    onClick={() => setEditing('new')}
                  >
                    New policy
                  </button>
                  {slotsLeft <= 0 && (
                    <p class="note">
                      The ceiling is {CEILING}, the implicit{' '}
                      <span class="mono">default</span> included — a rule&apos;s
                      policy membership is packed into 16 bits.
                    </p>
                  )}
                </div>
              </section>
            </div>

            <CostCard compiledRules={lists?.compiled_rules ?? null} />
            <CostFootnote />
          </>
        )}
      </main>

      {editing !== null && (
        <PolicyDialog
          policy={editing === 'new' ? null : editing}
          lists={lists?.items ?? []}
          busy={busy !== null}
          error={dialogError}
          suppressed={pending !== null}
          onSubmit={submit}
          onCancel={() => {
            setEditing(null);
            setDialogError(null);
          }}
        />
      )}

      {pending !== null && (
        <ConfirmDialog
          title={
            pending.create === null
              ? 'Change which lists this policy holds?'
              : 'Create this policy?'
          }
          confirmLabel={pending.create === null ? 'Change lists' : 'Create'}
          onCancel={() => setPending(null)}
          onConfirm={confirmPending}
        >
          Per-rule policy masks are built at compile time, so this rebuilds the
          whole compiled ruleset and swaps it in — seconds of CPU on the router,
          and the request waits for it. Renaming, the blocking-mode override and
          assignments are live in milliseconds and never ask.
        </ConfirmDialog>
      )}

      {removing !== null && (
        <ConfirmDialog
          title={`Delete ${removing.id}?`}
          confirmLabel="Delete"
          onCancel={() => setRemoving(null)}
          onConfirm={() => {
            const target = removing;
            setRemoving(null);
            run(target.id, true, () => deletePolicy(target.id));
          }}
        >
          Its {removing.assignments.length} assignment
          {removing.assignments.length === 1 ? '' : 's'} go with it, and every
          client they covered falls back to whatever else covers it — a subnet
          assignment or <span class="mono">default</span>. Deleting a policy
          rebuilds the whole compiled ruleset, so this takes seconds and the
          request waits for it.
        </ConfirmDialog>
      )}

      {busy?.recompiles === true && pending === null && (
        <BusyModal title="Rebuilding the ruleset">
          The policy set is being validated and, once accepted, the whole
          ruleset is recompiled and swapped in — seconds of CPU, and more on
          the router. It cannot be cancelled: stopping it part-way could leave
          the stored configuration ahead of the running matcher.
        </BusyModal>
      )}
    </>
  );
}

/**
 * `fah-config`'s `MAX_POLICIES`, which **includes** the implicit default — so
 * fifteen can be configured and the summary reads `N + 1 / 16`. It mirrors
 * `fah_model::PolicyId::MAX`, because a rule's policy membership is packed
 * into a `u16`.
 */
const CEILING = 16;

const DEFAULT_ID = 'default';

/** What is in flight, and whether it rebuilds the ruleset. One value, because
 *  the modal, the navigation block and the abort rule all key on the same
 *  fact and must not be able to disagree. */
interface Busy {
  label: string;
  recompiles: boolean;
}

export default Policies;
