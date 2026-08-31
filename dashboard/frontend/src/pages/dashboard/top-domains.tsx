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

/**
 * What a polled card's refresh cluster occupies, on a card that has no timer to
 * control. Without it the title bar is simply empty, and an empty title bar
 * reads as "this figure may be stale" rather than as "this figure arrives on
 * its own" — which is the question the two states are told apart by.
 *
 * The `conn live` classes are the top bar's, so the dot means the same thing in
 * both places. No `aria-live` here, unlike `ConnectionIndicator`: this marker
 * never changes, and a region that announces a word it will always say is noise
 * to a screen reader.
 */
function PushedLive() {
  return (
    <span class="ctl in-card">
      <span class="conn live">
        <span class="dot" />
        <span class="note">live</span>
      </span>
    </span>
  );
}
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
    <Card title={title} className={className} tools={<PushedLive />}>
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
