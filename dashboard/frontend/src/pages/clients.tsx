import { useCallback, useEffect, useMemo, useRef, useState } from 'preact/hooks';
import {
  clearClientPolicy,
  getClients,
  setClientName,
  setClientPolicy,
} from '../api/clients';
import { ApiError } from '../api/core';
import { getPolicies } from '../api/policies';
import type { Client, ClientPolicyBody, PoliciesResponse } from '../api/types';
import { Card } from '../components/card';
import { Chip } from '../components/chip';
import { EmptyState } from '../components/empty-state';
import { ErrorState } from '../components/error-state';
import { nowMs } from '../lifecycle/timers';
import { classifyAssignment } from '../policy/assignment';
import type { PageProps } from '../router/routes';
import { ContentHeader } from '../shell/content-header';

import { compareAddressesDesc, isV6 } from './clients/address';
import { AssignDialog } from './clients/assign-dialog';
import { ClientRow } from './clients/client-row';
import {
  EditingCard,
  TableFootnote,
  WhatThisIsNot,
} from './clients/legend-card';
import { RenameField } from './clients/rename-field';

/**
 * Every address that has actually asked something, and the policy in force for
 * it right now.
 *
 * **Two requests on mount, and never a third.** `GET /clients` carries `policy`
 * and `assignment_source` for every row — `p5-03` added them for this page —
 * so there is no per-row `GET /clients/{ip}/policy` and no accessor for one. A
 * household with forty observed clients would otherwise pay forty extra
 * requests on every page load. `GET /policies` is the second and last: it is
 * bounded at fifteen configured policies, it is needed anyway to populate the
 * assign-policy picker, and it carries the selectors and schedules the row
 * notes are worded from.
 *
 * **Both are re-read after every mutation, together.** The two responses are
 * cross-referenced, so they have to describe one instant: a rename can move a
 * client into or out of a name assignment, and an assignment moves a row
 * between policies inside `/policies`. Patching the local copy to mirror that
 * would be a second implementation of the resolver's bookkeeping.
 *
 * **Nothing here recompiles**, so nothing here confirms and nothing blocks —
 * the contrast with Policies' seconds is the point, not an implementation
 * detail. No timer, no event subscription, no clock: every "in force" and
 * "window shut" statement is read off `client.policy` rather than evaluated.
 */
type Family = 'all' | 'v4' | 'v6';

const FAMILIES: readonly { value: Family; label: string }[] = [
  { value: 'all', label: 'all' },
  { value: 'v4', label: 'IPv4' },
  { value: 'v6', label: 'IPv6' },
];

function familyOf(ip: string): Family {
  return isV6(ip) ? 'v6' : 'v4';
}

function familyLabel(family: Family): string {
  return FAMILIES.find((entry) => entry.value === family)?.label ?? family;
}

/**
 * The "none matching …" tail in one place, one shape per filter combination —
 * both filters, family only, search only, or (belt and braces) neither.
 */
function NoMatchDescription({
  family,
  needle,
}: {
  family: Family;
  needle: string;
}) {
  const familyPart = family === 'all' ? null : familyLabel(family);
  const needlePart = needle === '' ? null : <span class="mono">{needle}</span>;
  if (familyPart !== null && needlePart !== null) {
    return (
      <>
        {familyPart} {needlePart}
      </>
    );
  }
  return needlePart ?? <>{familyPart ?? 'the filters'}</>;
}

export function Clients(_props: PageProps) {
  const [clients, setClients] = useState<readonly Client[] | null>(null);
  const [policies, setPolicies] = useState<PoliciesResponse | null>(null);
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [mutationError, setMutationError] = useState<Error | null>(null);
  const [search, setSearch] = useState('');
  const [family, setFamily] = useState<Family>('all');
  const [openIp, setOpenIp] = useState<string | null>(null);
  const [renamingIp, setRenamingIp] = useState<string | null>(null);
  const [assigningIp, setAssigningIp] = useState<string | null>(null);
  const [busyIps, setBusyIps] = useState<ReadonlySet<string>>(new Set());
  const [now, setNow] = useState(nowMs);
  const controller = useRef<AbortController | null>(null);
  const disposed = useRef(false);

  /** Both, in parallel, every time. One instant, two views of it. */
  const load = useCallback(() => {
    controller.current?.abort();
    const next = new AbortController();
    controller.current = next;
    return Promise.all([
      getClients(next.signal),
      getPolicies(next.signal),
    ])
      .then(([clientList, policyList]) => {
        setClients(clientList.items);
        setPolicies(policyList);
        setNow(nowMs());
        setLoadError(null);
      })
      .catch((cause: unknown) => {
        if (cause instanceof DOMException && cause.name === 'AbortError') return;
        setLoadError(cause instanceof Error ? cause : new Error(String(cause)));
      });
  }, []);

  useEffect(() => {
    void load();
    return () => {
      disposed.current = true;
      controller.current?.abort();
    };
  }, [load]);

  /**
   * Every mutation is live in milliseconds and lands server-side even if the
   * page unmounts mid-flight, so none of them carries a signal and none of
   * them is optimistic: the row changes only after the re-read.
   */
  const run = useCallback(
    (ip: string, work: Promise<unknown>, silent404 = false) => {
      setBusyIps((current) => new Set(current).add(ip));
      setMutationError(null);
      work
        .catch((cause: unknown) => {
          // A 404 on the clear means nothing was assigned, which is the state
          // the operator asked for — the re-read settles it silently.
          if (silent404 && cause instanceof ApiError && cause.status === 404) {
            return;
          }
          setMutationError(
            cause instanceof Error ? cause : new Error(String(cause)),
          );
        })
        // The re-read runs whether or not the write succeeded: a 404 on either
        // path means this page's copy is stale, and that is exactly what the
        // re-read fixes. Except after unmount — a mutation answered once the
        // route is left must not issue requests attributable to a dead page;
        // the next entry re-reads anyway.
        .then(() => {
          if (!disposed.current) return load();
        })
        // Cleared per-ip, not wholesale: a second row's mutation can be in
        // flight, and the first one finishing must not wipe its busy state or
        // close its open editor. A set rather than one slot, so both rows read
        // busy while both are in flight.
        .finally(() => {
          setBusyIps((current) => {
            const next = new Set(current);
            next.delete(ip);
            return next;
          });
          setRenamingIp((current) => (current === ip ? null : current));
          setAssigningIp((current) => (current === ip ? null : current));
        });
    },
    [load],
  );

  const rename = useCallback(
    (client: Client, name: string | null) => {
      run(client.ip, setClientName(client.ip, name));
    },
    [run],
  );

  const assign = useCallback(
    (client: Client, body: ClientPolicyBody) => {
      run(client.ip, setClientPolicy(client.ip, body));
    },
    [run],
  );

  const clear = useCallback(
    (client: Client) => {
      run(client.ip, clearClientPolicy(client.ip), true);
    },
    [run],
  );

  const items = clients ?? [];
  const policyItems = policies?.items ?? [];

  /**
   * Family filter, then search, then order — and the order is **descending by
   * address**, on the numeric key rather than the string
   * ([`addressKey`](./clients/address)).
   *
   * The filter earns its place because the registry keys on the address, not
   * the device ([`client_registry.rs`](../../../crates/fah-stats/src/client_registry.rs)):
   * one dual-stack machine is two rows, and on a dual-stack LAN that doubles
   * the table for no new information.
   */
  const visible = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return items
      .filter((client) => family === 'all' || familyOf(client.ip) === family)
      .filter(
        (client) =>
          needle === '' ||
          client.ip.toLowerCase().includes(needle) ||
          (client.name ?? '').toLowerCase().includes(needle),
      )
      .sort((left, right) => compareAddressesDesc(left.ip, right.ip));
  }, [items, search, family]);

  const assigning = items.find((client) => client.ip === assigningIp) ?? null;

  return (
    <>
      <ContentHeader
        title="Clients"
        context={
          <>
            Every address that has actually asked something —{' '}
            <span class="mono">GET /api/v1/clients</span>
          </>
        }
      />

      <main class="wrap">
        {mutationError !== null && <ErrorState error={mutationError} />}
        {/* A failed re-read after the first successful load: the rows below
            are stale and silence would hide it. */}
        {loadError !== null && clients !== null && (
          <ErrorState error={loadError} />
        )}

        <Card
          title="Observed clients"
          tools={
            <>
              <span class="chips" role="group" aria-label="Filter by address family">
                {FAMILIES.map(({ value, label }) => (
                  <Chip
                    key={value}
                    label={label}
                    on={family === value}
                    onPick={() => setFamily(value)}
                  />
                ))}
              </span>
              <input
                type="search"
                class="field-input search-input"
                placeholder="search address or name…"
                aria-label="Search clients by address or name"
                value={search}
                onInput={(event) => setSearch(event.currentTarget.value)}
              />
            </>
          }
          bodyClass="clients-body"
        >
          {loadError !== null && clients === null ? (
            <ErrorState error={loadError} />
          ) : items.length === 0 ? (
            <EmptyState title="No client has asked anything yet">
              Clients appear here as they send their first query — there is no
              inventory to read from.
            </EmptyState>
          ) : visible.length === 0 ? (
            <EmptyState title="No client matches those filters">
              {items.length} observed{' '}
              {items.length === 1 ? 'client' : 'clients'}, none matching{' '}
              <NoMatchDescription family={family} needle={search.trim()} />.
            </EmptyState>
          ) : (
            <div class="clients-scroll">
              <div class="clients-table">
                <div class="client-head" aria-hidden="true">
                  <span class="c-ip">Address</span>
                  <span class="c-name">Name</span>
                  <span class="c-policy">Policy in force</span>
                  <span class="c-queries">Queries 24 h</span>
                  <span class="c-blocked">Blocked</span>
                  <span class="c-share">Blocked share</span>
                  <span class="c-seen">Last seen</span>
                  <span class="c-actions" />
                </div>
                {visible.map((client) => (
                  <ClientRow
                    key={client.ip}
                    client={client}
                    classification={classifyAssignment(client, policyItems)}
                    now={now}
                    open={openIp === client.ip}
                    busy={busyIps.has(client.ip)}
                    onToggle={() => {
                      setRenamingIp(null);
                      setOpenIp((current) =>
                        current === client.ip ? null : client.ip,
                      );
                    }}
                  >
                    {renamingIp === client.ip ? (
                      <RenameField
                        client={client}
                        busy={busyIps.has(client.ip)}
                        onSave={(name) => rename(client, name)}
                        onCancel={() => setRenamingIp(null)}
                      />
                    ) : (
                      <div class="row-expand-actions">
                        <button
                          type="button"
                          class="btn g"
                          disabled={busyIps.has(client.ip)}
                          onClick={() => setRenamingIp(client.ip)}
                        >
                          Rename
                        </button>
                        <button
                          type="button"
                          class="btn"
                          disabled={
                            busyIps.has(client.ip) || policies === null
                          }
                          onClick={() => setAssigningIp(client.ip)}
                        >
                          Change policy
                        </button>
                      </div>
                    )}
                  </ClientRow>
                ))}
              </div>
            </div>
          )}
          <TableFootnote />
        </Card>

        <div class="row c2">
          <EditingCard />
          <WhatThisIsNot />
        </div>
      </main>

      {assigning !== null && (
        <AssignDialog
          client={assigning}
          policies={policyItems}
          busy={busyIps.has(assigning.ip)}
          onAssign={(body) => assign(assigning, body)}
          onClear={() => clear(assigning)}
          onCancel={() => setAssigningIp(null)}
        />
      )}
    </>
  );
}

export default Clients;
