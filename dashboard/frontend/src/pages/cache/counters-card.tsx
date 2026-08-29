import type { CacheUsage } from '../../api/types';
import { percent1 } from '../../charts/format';
import { Card } from '../../components/card';
import { Donut } from '../../components/donut';
import { EmptyState } from '../../components/empty-state';
import { cacheHitRate, cacheLookups } from '../../derive';
import { CounterTable } from './counter-table';

/**
 * `hits`, `misses` and `evictions` are process-lifetime counters read verbatim.
 * `lookups` is E3 — `hits + misses`, which is resolved queries (`pass + allow`)
 * because a blocked query never reaches the cache at all (ADR-0001).
 */
export function CountersCard({ cache }: { cache: CacheUsage | null }) {
  return (
    <Card title="Lifetime counters" secondary="since process start">
      {cache === null ? (
        <EmptyState title="Not read yet" />
      ) : (
        <>
          <CounterTable
            rows={[
              { label: 'lookups', value: cacheLookups(cache.hits, cache.misses).toLocaleString() },
              { label: 'hits', value: cache.hits.toLocaleString(), tone: 'good' },
              { label: 'misses', value: cache.misses.toLocaleString() },
              { label: 'evictions', value: cache.evictions.toLocaleString() },
            ]}
          />
          <HitRate cache={cache} />
        </>
      )}
    </Card>
  );
}

function HitRate({ cache }: { cache: CacheUsage }) {
  // E4 — a zero denominator draws the empty track rather than dividing.
  const rate = cacheHitRate(cache.hits, cache.misses);
  return (
    <div class="hit-rate">
      <div class="hit-ring">
        <Donut
          size={86}
          thickness={14}
          label={`Cache hit rate, ${percent1(rate)} percent`}
          segments={[
            { label: 'hits', value: cache.hits, colour: 'var(--cache-fresh)' },
            // Painted in the track's own colour: the ring's remainder is the
            // misses, and drawing it as a second band would give a figure the
            // legend beside it already prints.
            { label: 'misses', value: cache.misses, colour: 'var(--track)' },
          ]}
        />
        <span class="hit-figure mono">{percent1(rate)}%</span>
      </div>
      <p class="note">
        Hit rate. Every hit is a query that never left the box — the difference
        between answering from memory and an upstream round trip.
      </p>
    </div>
  );
}
