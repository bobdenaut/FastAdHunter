import type { Client } from '../../api/types';
import { percent1 } from '../../charts/format';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { RefreshCluster } from '../../components/refresh-cluster';
import { blockedPercent, shareOfMax } from '../../derive';
import type { RefreshRegistry } from '../../refresh/registry';
import { Link } from '../../router/link';
import { TopList } from './top-list';

const SHOWN = 5;

/**
 * **`GET /clients`, not `/stats.top_clients`.** Both artboards draw a blocked
 * figure beside the query count — a count on the desktop table and a percentage
 * on the phone — and `/stats.top_clients` carries `{ip, name, count}` and no
 * blocked figure at all. Reading the whole card from one response also means
 * the pair comes from one snapshot rather than two.
 *
 * R9 is the share bar (`queries_24h / max`), R10 the phone's blocked percentage
 * (`blocked_24h / queries_24h`). The desktop Blocked column is `blocked_24h`
 * verbatim.
 *
 * The refresh cluster is deviation **X3**: D1a gave this card a polled endpoint
 * and therefore an interval an operator should be able to set. The artboards
 * were drawn when the card was a mount snapshot.
 */
export function TopClients({
  clients,
  registry,
  className,
}: {
  clients: readonly Client[] | undefined;
  registry: RefreshRegistry;
  className?: string;
}) {
  const ranked = [...(clients ?? [])]
    .sort((a, b) => b.queries_24h - a.queries_24h)
    .slice(0, SHOWN);
  const max = ranked.reduce((top, row) => Math.max(top, row.queries_24h), 0);

  return (
    <Card
      title="Top clients"
      secondary="by queries"
      tools={<RefreshCluster registry={registry} endpoint="clients" />}
      className={className}
    >
      {ranked.length === 0 ? (
        <EmptyState title="No clients yet">
          No address has resolved anything through this box in the last 24
          hours.
        </EmptyState>
      ) : (
        <TopList
          variant="clients"
          columns={['Client', 'Queries', 'Blocked']}
          frequencyLabel="Share"
          rows={ranked.map((row) => ({
            key: row.ip,
            primary: (
              <span class={row.name === null ? 'mono' : 'mono has-name'}>
                <span class="ip">{row.ip}</span>
                {row.name !== null && <span class="cname">{row.name}</span>}
              </span>
            ),
            count: row.queries_24h,
            extra: (
              <>
                <span class="x-long">{row.blocked_24h.toLocaleString()}</span>
                <span class="x-short">
                  {percent1(blockedPercent(row.queries_24h, row.blocked_24h))}%
                </span>
              </>
            ),
            share: shareOfMax(row.queries_24h, max),
          }))}
          more={<Link href="/clients">Open Clients</Link>}
        />
      )}
    </Card>
  );
}
