import type { CacheUsage } from '../../api/types';
import { formatMiB, percent1 } from '../../charts/format';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { closestBound, type CacheBound } from '../../derive';

const CALLOUT: Record<CacheBound, preact.ComponentChildren> = {
  entries: <b>Entries is the bound closest to evicting</b>,
  bytes: <b>Bytes is the bound closest to evicting</b>,
  equal: <b>Both bounds are equally loaded</b>,
};

/**
 * Both percentages are served fields, rendered verbatim. The only derived thing
 * on this card is E5 — which of the two names itself in the callout — and it is
 * a comparison, not a recomputation: eviction runs oldest-first until entries
 * **and** bytes are each back inside their bound, so the higher load is the one
 * that triggers first (API.md §Cache).
 */
export function BoundsCard({ cache }: { cache: CacheUsage | null }) {
  return (
    <Card
      title="The two bounds"
      secondary="eviction runs until both are satisfied"
    >
      {cache === null ? (
        <EmptyState title="Not read yet" />
      ) : (
        <>
          <Bound
            name="entries"
            detail={`${cache.entries.toLocaleString()} / ${cache.capacity.toLocaleString()}`}
            percent={cache.load_percent}
            colour="var(--accent)"
          />
          <Bound
            name="bytes"
            detail={`${formatMiB(cache.bytes)} / ${formatMiB(cache.max_bytes)}`}
            percent={cache.byte_load_percent}
            colour="var(--tier-url)"
          />
          <p class="callout">
            {CALLOUT[closestBound(cache.load_percent, cache.byte_load_percent)]}{' '}
            — the higher of the two is always the one that will trigger first.
          </p>
          <p class="note">
            <span class="mono">bytes</span> is a coarse per-entry estimate of
            what resident answers hold. It excludes the hash-table slabs, which
            the memory breakdown counts separately — the two figures are
            answering different questions, not disagreeing.
          </p>
        </>
      )}
    </Card>
  );
}

function Bound({
  name,
  detail,
  percent,
  colour,
}: {
  name: string;
  detail: string;
  percent: number;
  colour: string;
}) {
  return (
    <div class="bound">
      <div class="bound-head">
        <span>
          <b>{name}</b> <span class="note">{detail}</span>
        </span>
        <span class="mono bound-figure">{percent1(percent)} %</span>
      </div>
      <div class="bar bound-bar">
        <span
          style={{
            width: `${String(Math.max(0, Math.min(100, percent)))}%`,
            background: colour,
          }}
        />
      </div>
    </div>
  );
}
