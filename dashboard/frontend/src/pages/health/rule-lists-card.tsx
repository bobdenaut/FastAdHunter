import type { ListsCounters, ListsResponse } from '../../api/types';
import { formatBytes } from '../../charts/format';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { RefreshCluster } from '../../components/refresh-cluster';
import { StatusPill } from '../../components/status-pill';
import { listsNeedingAttention } from '../../derive';
import { Link } from '../../router/link';
import { refresh } from '../../services';

/**
 * D4 — the lists that need attention, and how many do not.
 *
 * Neither state is an outage: a **failed** refresh keeps the previous copy
 * serving, and a **rejected** one was refused by the content gate before it
 * could commit. Filtering is unaffected either way, and the card says so rather
 * than colouring the page red.
 *
 * `counters` is `telemetry.counters.lists` — the route already polls
 * `/telemetry`, so the line costs zero new reads.
 */
export function RuleListsCard({
  lists,
  counters,
}: {
  lists: ListsResponse | null;
  counters: ListsCounters | null;
}) {
  const items = lists?.items ?? null;
  const problems = items === null ? [] : listsNeedingAttention(items);

  return (
    <Card
      title="Rule lists"
      secondary={
        items === null
          ? undefined
          : `${problems.length} of ${items.length} need attention`
      }
      tools={<RefreshCluster registry={refresh} endpoint="lists" />}
    >
      {items === null ? (
        <EmptyState title="Not read yet" />
      ) : problems.length === 0 ? (
        <p class="note">
          All {items.length} {items.length === 1 ? 'list is' : 'lists are'}{' '}
          refreshing normally.
        </p>
      ) : (
        <>
          <div class="health-problems">
            {problems.map((item) => (
              <div class="health-problem" key={item.id}>
                <span class="mono">{item.id}</span>
                <StatusPill status={item.last_status} />
                <span class="note">
                  {item.last_error ?? 'no reason reported'}
                </span>
              </div>
            ))}
          </div>
          <p class="note">
            {items.length - problems.length}{' '}
            {items.length - problems.length === 1 ? 'other is' : 'others are'} ok.
          </p>
        </>
      )}
      {counters !== null && (
        <p class="note">
          {counters.bodies.toLocaleString()}{' '}
          {counters.bodies === 1 ? 'refresh' : 'refreshes'} downloaded a body (
          {formatBytes(counters.bytes_fetched)} total),{' '}
          {counters.not_modified.toLocaleString()} answered 304 — unchanged
          lists answer 304 and cost no download, no recompile.
        </p>
      )}
      <p class="note">
        Neither is an outage: a failed refresh keeps the previous copy serving,
        and a rejected one was refused by the content gate before it could
        commit. Filtering is unaffected.{' '}
        <Link href="/lists" class="linky">
          Open Lists →
        </Link>
      </p>
    </Card>
  );
}
