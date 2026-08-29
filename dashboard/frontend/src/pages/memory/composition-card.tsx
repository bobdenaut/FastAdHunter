import type { DebugMemory } from '../../api/types';
import { compactCount, formatMiB } from '../../charts/format';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { sliceShare, statsBytes } from '../../derive';

interface Component {
  key: string;
  label: string;
  bytes: number;
  detail: string;
  /** The bar segment's class. Residual takes the hatch and no hue. */
  tone: 'ruleset' | 'cache' | 'stats' | 'residual';
}

/**
 * What RSS is made of, this instant — a 100 %-stacked bar over four tiles.
 *
 * **The four sum to RSS exactly, and that is a server-side identity rather than
 * a client-side rounding.** `residual_bytes` is `process_rss − accounted_bytes`
 * computed where both numbers are read, so it is a measured slice, not this
 * page's estimate of what is left over. Which is why it is drawn as a texture:
 * it is real, and it is not one structure — binary text and data pages, thread
 * stacks, the Tokio runtime, and memory the allocator holds but has not
 * returned. A hue would promise it is a thing.
 *
 * **Peak is deliberately not a segment.** A lifetime high-water mark is not part
 * of current RSS; putting it in the bar would draw a whole that does not exist
 * and halve the ruleset's apparent share. It lives on the KPI rail, which is
 * what a high-water mark is for.
 *
 * **Cache and stats share one hue in two steps.** They are 2 % and 3 % of RSS —
 * segments a few pixels wide — and a fourth categorical hue does not survive the
 * colour-blind separation floor beside the other three plus the amber and the
 * red. Lightness separates them here; the labels and figures carry the identity.
 */
export function CompositionCard({
  memory,
  rules = null,
}: {
  memory: DebugMemory | null;
  /**
   * `telemetry.ruleset.rules`, which `/debug/memory` does not carry. Optional
   * and nullable because it comes from the page's third read: when that read
   * failed the tile says `compiled rules` and loses a figure, never the tile.
   */
  rules?: number | null;
}) {
  const rss = memory?.process_rss ?? null;
  const residual = memory?.residual_bytes ?? null;

  if (memory === null || rss === null || residual === null) {
    return (
      <Card title="Memory composition · RSS">
        <EmptyState title="RSS unavailable on this platform">
          <span class="mono">process_rss</span> is read from{' '}
          <span class="mono">/proc/self/status</span> and is null off Linux, so
          there is no total for the components to sum to.
        </EmptyState>
      </Card>
    );
  }

  // `fah-model`'s `over_accounted()`, read on this side: residual is a
  // `saturating_sub`, so a zero residual beside an `accounted` above RSS is the
  // floor doing its job, not a process with nothing unaccounted for. The card
  // states which of the two it is drawing rather than claiming the identity
  // holds in both.
  const overAccounted = memory.accounted_bytes > rss;

  const components: Component[] = [
    {
      key: 'ruleset',
      label: 'Ruleset & data',
      bytes: memory.ruleset_bytes,
      detail: rules === null ? 'compiled rules' : `${rules.toLocaleString()} rules`,
      tone: 'ruleset',
    },
    {
      key: 'residual',
      label: 'Residual / unaccounted',
      bytes: residual,
      detail: 'not one structure',
      tone: 'residual',
    },
    {
      key: 'cache',
      label: 'DNS cache',
      bytes: memory.cache_estimated_bytes,
      detail: `${compactCount(memory.cache_entries)} entries`,
      tone: 'cache',
    },
    {
      key: 'stats',
      label: 'Stats + clients',
      bytes: statsBytes(memory),
      detail: 'aggregates',
      tone: 'stats',
    },
  ];

  // Bar order is the stacking order, bottom-up, which is not the tile order:
  // the tiles lead with the two that matter, the bar has to stay a stack.
  const bar: Component[] = ['ruleset', 'cache', 'stats', 'residual'].flatMap(
    (key) => components.filter((component) => component.key === key),
  );

  return (
    <Card
      title="Memory composition · RSS"
      secondary={`total ${formatMiB(rss)}`}
      className="composition"
    >
      <div class="composition-bar">
        {bar.map((component) => (
          <div
            key={component.key}
            class={`composition-seg seg-${component.tone}`}
            style={`width: ${sliceShare(component.bytes, rss).toFixed(2)}%`}
            title={`${component.label} — ${formatMiB(component.bytes)}`}
          >
            {sliceShare(component.bytes, rss) >= 10 && (
              <span class="num">
                {sliceShare(component.bytes, rss).toFixed(1)} %
              </span>
            )}
          </div>
        ))}
      </div>

      <div class="composition-tiles">
        {components.map((component) => (
          <div class="composition-tile" key={component.key}>
            <div class="tl">
              <span class={`sw sw-${component.tone}`} />
              {component.label}
            </div>
            <div class={`tv num tv-${component.tone}`}>
              {formatMiB(component.bytes).split(' ')[0]}{' '}
              <span class="tu">MiB</span>
            </div>
            <div class="note composition-detail">
              {sliceShare(component.bytes, rss).toFixed(1)} % · {component.detail}
            </div>
          </div>
        ))}
      </div>

      <div class="composition-accounted">
        <span>accounted</span>
        <span class="num">
          {formatMiB(memory.accounted_bytes)} ·{' '}
          {sliceShare(memory.accounted_bytes, rss).toFixed(1)} %
        </span>
      </div>

      {overAccounted ? (
        <p class="note composition-note">
          <b>These components claim more than RSS</b>, which is an accounting
          bug and never a real state — a component is double-counting or
          counting something that is not resident. Residual is floored at zero
          server-side rather than wrapping, so the shares above do not sum to
          100 %: read the figures, not the bar.
        </p>
      ) : (
        <p class="note composition-note">
          The four components <b>sum to RSS exactly</b> — residual is{' '}
          <span class="mono">process_rss − accounted_bytes</span>, a measured
          slice rather than an estimate of what is left over. It is hatched
          because it is not one structure: binary text and data pages, thread
          stacks, the Tokio runtime, and memory the allocator holds but has not
          returned to the kernel.
        </p>
      )}
    </Card>
  );
}
