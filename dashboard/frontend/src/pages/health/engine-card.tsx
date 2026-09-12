import type { Telemetry } from '../../api/types';
import { formatMiB } from '../../charts/format';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { Link } from '../../router/link';

/**
 * The ruleset, the footprint and the supervised-task death count, from
 * `/telemetry`.
 *
 * `memory.process_rss` and `process_peak_rss` are `null` off Linux — there is
 * no `/proc/self/status` on a non-container dev box — so they render as
 * unavailable rather than as a zero-byte process.
 *
 * `counters.tasks_died` is the one figure here that is allowed to go red: a
 * scheduler, the event fan-out, the perf sampler or an SWR worker ended before
 * shutdown. The resolver keeps answering, but that task's work (list refresh,
 * stats, history, SWR) has stopped until the container is restarted; the
 * engine log carries the task name and the panic message.
 */
export function EngineCard({ telemetry }: { telemetry: Telemetry | null }) {
  if (telemetry === null) {
    return (
      <Card title="Engine">
        <EmptyState title="Not read yet" />
      </Card>
    );
  }

  const { ruleset, memory, counters } = telemetry;
  const deaths = counters.tasks_died;

  return (
    <Card title="Engine">
      <div class="figure-grid">
        <div>
          <div class="figure mono">{ruleset.rules.toLocaleString()}</div>
          <div class="note">compiled rules</div>
        </div>
        <div>
          <div class="figure mono">
            {ruleset.duplicates_removed.toLocaleString()}
          </div>
          <div class="note">duplicates removed</div>
        </div>
        <div>
          <div class="figure mono">
            {ruleset.compile_duration_seconds.toFixed(2)} s
          </div>
          <div class="note">last compile</div>
        </div>
        <div>
          <div class="figure mono">{bytes(memory.process_rss)}</div>
          <div class="note">resident memory</div>
        </div>
        <div>
          <div class="figure mono">{bytes(memory.process_peak_rss)}</div>
          <div class="note">peak, this process</div>
        </div>
        <div>
          <div class={deaths > 0 ? 'figure mono bad' : 'figure mono'}>
            {deaths.toLocaleString()}
          </div>
          <div class="note">supervised tasks died</div>
        </div>
      </div>
      <p class="note">
        Full breakdown, leak watch and fault rates{' '}
        <Link href="/diagnostics/memory" class="linky">
          Open Memory →
        </Link>
      </p>
    </Card>
  );
}

/** `null` is "unavailable off Linux", never zero. */
function bytes(value: number | null): string {
  return value === null ? 'unavailable' : formatMiB(value);
}
