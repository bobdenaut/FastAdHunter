import type { CacheUsage } from '../../api/types';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { StageBar } from '../../components/stage-bar';
import { freeEntries } from '../../derive';

interface Stage {
  label: string;
  colour: string;
  value: (cache: CacheUsage) => number;
  note: preact.ComponentChildren;
}

/**
 * Each stage's meaning travels with its figure — the colours are a second
 * signal, never the only one, and the four notes are what turn a proportion
 * bar into an answer.
 *
 * The artboard prints a latency figure inside the `fresh` and hit-rate notes.
 * Dropped: the only same-page source would be `telemetry.latency`'s lifetime
 * `sum / count`, which API.md warns is meaningless and which this task forbids
 * everywhere. The sentence keeps its meaning without a numeral.
 */
const STAGES: readonly Stage[] = [
  {
    label: 'fresh',
    colour: 'var(--cache-fresh)',
    value: (cache) => cache.fresh,
    note: 'Inside its TTL. Answers the client directly, without leaving the box.',
  },
  {
    label: 'stale',
    colour: 'var(--cache-stale)',
    value: (cache) => cache.stale,
    note: (
      <>
        Past TTL, inside the RFC 8767 serve-stale window. Answers <b>only</b>{' '}
        after a forward has failed — this is outage insurance, not waste.
      </>
    ),
  },
  {
    label: 'expired',
    colour: 'var(--cache-expired)',
    value: (cache) => cache.expired,
    note: 'Past the stale window. Dead weight waiting for eviction or a clean.',
  },
  {
    label: 'free',
    colour: 'var(--cache-free)',
    // E1 — `capacity − entries`, floored at zero and labelled `free` rather
    // than dressed up as a measured band.
    value: (cache) => freeEntries(cache.capacity, cache.entries),
    note: 'Capacity is per-shard × shard count, so it can round slightly below the configured maximum.',
  },
];

export function StageCard({ cache }: { cache: CacheUsage | null }) {
  return (
    <Card
      title="Entries by lifetime stage"
      secondary={
        cache === null
          ? undefined
          : // E2 — formatting of `entries` and `capacity`.
            `${cache.entries.toLocaleString()} of ${cache.capacity.toLocaleString()}`
      }
      className="cache-stage"
    >
      {cache === null ? (
        <EmptyState title="Not read yet" />
      ) : (
        <>
          <StageBar
            segments={STAGES.map((stage) => ({
              label: stage.label,
              value: stage.value(cache),
              colour: stage.colour,
            }))}
          />
          <div class="stage-notes">
            {STAGES.map((stage) => (
              <div key={stage.label}>
                <div class="stage-name">
                  <span class="sw" style={{ background: stage.colour }} />
                  {stage.label}{' '}
                  <span class="mono">
                    {stage.value(cache).toLocaleString()}
                  </span>
                </div>
                <p class="note">{stage.note}</p>
              </div>
            ))}
          </div>
        </>
      )}
    </Card>
  );
}
