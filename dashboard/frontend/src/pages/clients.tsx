import { useCallback, useEffect, useMemo, useRef, useState } from 'preact/hooks';
import {
  clearClientPolicy,
  getClients,
  setClientName,
  setClientPolicy,
} from '../api/clients';
import { ApiError } from '../api/core';
import { getPolicies } from '../api/policies';
import type {
  Client,
  ClientFamily,
  ClientPolicyBody,
  PoliciesResponse,
} from '../api/types';
import { Card } from '../components/card';
import { Chip } from '../components/chip';
import { EmptyState } from '../components/empty-state';
import { ErrorState } from '../components/error-state';
import { blockedPercent } from '../derive';
import { nowMs } from '../lifecycle/timers';
import { classifyAssignment } from '../policy/assignment';
import type { PageProps } from '../router/routes';
import { ContentHeader } from '../shell/content-header';

import { addressKey, compareKeys } from './clients/address';
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
 * **A family or last-seen chip re-reads, search does not.** The registry keys
 * on the address, not the device, so one dual-stack machine is two rows and
 * its IPv6 privacy addresses churn through as more. `?family=` and
 * `?seen_within=` leave that on the server, and the page opens on IPv4 seen in
 * the last 24 h so it is the length of the household rather than of the
 * address space. Search narrows the rows already here.
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
type Family = 'all' | ClientFamily;

type Seen = 'all' | '24h';

type SortColumn = 'address' | 'queries' | 'blocked' | 'share';

type SortDirection = 'asc' | 'desc';

interface Sort {
  column: SortColumn;
  direction: SortDirection;
}

const OPENING_DIRECTION: Record<SortColumn, SortDirection> = {
  address: 'asc',
  queries: 'desc',
  blocked: 'desc',
  share: 'desc',
};

const SORT_KEYS: Record<
  Exclude<SortColumn, 'address'>,
  (client: Client) => number
> = {
  queries: (client) => client.queries_24h,
  blocked: (client) => client.blocked_24h,
  share: (client) => blockedPercent(client.queries_24h, client.blocked_24h),
};

const HEAD: readonly {
  cell: string;
  label: string;
  column: SortColumn | null;
}[] = [
  { cell: 'c-ip', label: 'Address', column: 'address' },
  { cell: 'c-name', label: 'Name', column: null },
  { cell: 'c-policy', label: 'Policy in force', column: null },
  { cell: 'c-queries', label: 'Queries 24 h', column: 'queries' },
  { cell: 'c-blocked', label: 'Blocked', column: 'blocked' },
  { cell: 'c-ratio', label: 'Blocked share', column: 'share' },
  { cell: 'c-seen', label: 'Last seen', column: null },
];

const FAMILIES: readonly { value: Family; label: string; noun: string }[] = [
  { value: 'all', label: 'all', noun: 'client' },
  { value: 'v4', label: 'IPv4', noun: 'IPv4 client' },
  { value: 'v6', label: 'IPv6', noun: 'IPv6 client' },
];

const SEEN: readonly { value: Seen; label: string }[] = [
  { value: '24h', label: 'last 24 h' },
  { value: 'all', label: 'all time' },
];

/** "client", "IPv4 clients" — the empty states name the family they read. */
function clientNoun(family: Family, count: number): string {
  const noun =
    FAMILIES.find((entry) => entry.value === family)?.noun ?? 'client';
  return count === 1 ? noun : `${noun}s`;
}

function SortCaret({
  direction,
  active,
}: {
  direction: SortDirection;
  active: boolean;
}) {
  return (
    <>
      <span
        class={active ? 'sort-caret' : 'sort-caret is-idle'}
        aria-hidden="true"
      >
        {direction === 'asc' ? '▲' : '▼'}
      </span>
      {active && (
        <span class="visually-hidden">
          {direction === 'asc' ? ', ascending' : ', descending'}
        </span>
      )}
    </>
  );
}

export function Clients(_props: PageProps) {
  const [clients, setClients] = useState<readonly Client[] | null>(null);
  const [policies, setPolicies] = useState<PoliciesResponse | null>(null);
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [mutationError, setMutationError] = useState<Error | null>(null);
  const [search, setSearch] = useState('');
  const [family, setFamily] = useState<Family>('v4');
  const [seen, setSeen] = useState<Seen>('24h');
  const [sort, setSort] = useState<Sort>({
    column: 'address',
    direction: 'asc',
  });
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
      getClients(
        next.signal,
        family === 'all' ? undefined : family,
        seen === 'all' ? undefined : seen,
      ),
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
  }, [family, seen]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(
    () => () => {
      disposed.current = true;
      controller.current?.abort();
    },
    [],
  );

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

  const sortBy = useCallback((column: SortColumn) => {
    setSort((current) =>
      current.column === column
        ? { column, direction: current.direction === 'asc' ? 'desc' : 'asc' }
        : { column, direction: OPENING_DIRECTION[column] },
    );
  }, []);

  const items = clients ?? [];
  const policyItems = policies?.items ?? [];

  /**
   * Search, then order. The order is whichever column header was last
   * clicked — **ascending by address** to open with, on the numeric key rather
   * than the string ([`addressKey`](./clients/address)). The three number
   * columns sort on the figure the row draws, share included
   * ([`blockedPercent`](../derive) is the one that draws it), and every header
   * toggles its own direction.
   *
   * **The address key is built once per row, in `keyed` above.** It is memoised
   * on the read rather than on the sort, so clicking a header re-sorts without
   * rebuilding a single key; the comparator only ever compares two strings
   * ([`compareKeys`](./clients/address) says what that cost).
   *
   * **The tie-break is always the address, ascending, whichever way the
   * primary key points.** Two clients on the same count would otherwise swap
   * places between reads, and reversing the tie-break with the column would
   * make a descending list read as a different set of rows rather than the
   * same ones upside down. The family is already narrowed by the read
   * ([`client_registry.rs`](../../../crates/fah-stats/src/client_registry.rs)
   * keys on the address, not the device).
   */
  const keyed = useMemo(
    () => items.map((client) => ({ client, key: addressKey(client.ip) })),
    [items],
  );

  const visible = useMemo(() => {
    const needle = search.trim().toLowerCase();
    const count = sort.column === 'address' ? null : SORT_KEYS[sort.column];
    const descending = sort.direction === 'desc';
    return keyed
      .filter(
        ({ client }) =>
          needle === '' ||
          client.ip.toLowerCase().includes(needle) ||
          (client.name ?? '').toLowerCase().includes(needle),
      )
      .sort((left, right) => {
        const primary =
          count === null
            ? compareKeys(left.key, right.key)
            : count(left.client) - count(right.client);
        const directed = descending ? -primary : primary;
        return directed || compareKeys(left.key, right.key);
      })
      .map(({ client }) => client);
  }, [keyed, search, sort]);

  const assigning = items.find((client) => client.ip === assigningIp) ?? null;

  const familyChips = (
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
  );

  const seenChips = (
    <span class="chips" role="group" aria-label="Filter by last seen">
      {SEEN.map(({ value, label }) => (
        <Chip
          key={value}
          label={label}
          on={seen === value}
          onPick={() => setSeen(value)}
        />
      ))}
    </span>
  );

  const emptyTitle =
    seen === '24h'
      ? `No ${clientNoun(family, 1)} has asked anything in the last 24 h`
      : `No ${clientNoun(family, 1)} has asked anything yet`;

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
          title={
            <>
              Observed clients
              <span class="chips-mobile">
                {familyChips}
                {seenChips}
              </span>
            </>
          }
          tools={
            <>
              {familyChips}
              {seenChips}
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
            <EmptyState title={emptyTitle}>
              Clients appear here as they send their first query — there is no
              inventory to read from.
            </EmptyState>
          ) : visible.length === 0 ? (
            <EmptyState title="No client matches that search">
              {items.length} observed {clientNoun(family, items.length)}, none
              matching <span class="mono">{search.trim()}</span>.
            </EmptyState>
          ) : (
            <div class="clients-scroll">
              <div class="clients-table">
                <div class="client-head">
                  {HEAD.map(({ cell, label, column }) =>
                    column === null ? (
                      <span key={cell} class={cell}>
                        {label}
                      </span>
                    ) : (
                      <button
                        key={cell}
                        type="button"
                        class={`${cell} sort-head`}
                        aria-pressed={sort.column === column}
                        onClick={() => sortBy(column)}
                      >
                        {label}
                        <SortCaret
                          active={sort.column === column}
                          direction={
                            sort.column === column
                              ? sort.direction
                              : OPENING_DIRECTION[column]
                          }
                        />
                      </button>
                    ),
                  )}
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
