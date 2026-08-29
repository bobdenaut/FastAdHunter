import { useState } from 'preact/hooks';
import type { TopDomain } from '../../api/types';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { shareOfMax } from '../../derive';
import { TopList } from './top-list';

/**
 * R8 — the frequency bar is `count / max(count)` over the rendered rows, the
 * normalisation `Main.dc.html` settles with its own figures (3,140 / 4,021 =
 * 78 %). Both tables come straight from `/stats`, so they move with the socket
 * push.
 */
export function TopDomains({
  title,
  domains,
  tone,
  className,
}: {
  title: string;
  domains: readonly TopDomain[] | undefined;
  tone?: 'blocked';
  className?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const max = domains?.reduce((top, row) => Math.max(top, row.count), 0) ?? 0;

  return (
    <Card title={title} className={className}>
      {domains === undefined || domains.length === 0 ? (
        <EmptyState title="Nothing yet">
          No queries have been recorded in the last 24 hours.
        </EmptyState>
      ) : (
        <TopList
          columns={['Domain', 'Hits']}
          expanded={expanded}
          rows={domains.map((row) => ({
            key: row.domain,
            primary: <span class="mono">{row.domain}</span>,
            count: row.count,
            share: shareOfMax(row.count, max),
            ...(tone === undefined ? {} : { tone }),
          }))}
          more={
            <button
              type="button"
              class="linky"
              onClick={() => setExpanded((open) => !open)}
            >
              {expanded ? 'Show fewer' : `Show all ${String(domains.length)}`}
            </button>
          }
        />
      )}
    </Card>
  );
}
