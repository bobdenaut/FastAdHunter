import type { Telemetry } from '../../api/types';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { RefreshCluster } from '../../components/refresh-cluster';
import { refresh } from '../../services';

/**
 * What clients actually received — `counters.dns.answers`, verbatim.
 *
 * The split is the point: **synthesized** means FastAdHunter could not get an
 * answer and said so; **relayed** means an upstream answered with an error and
 * FastAdHunter passed it on unchanged. The first points here, the second points
 * at the resolver being used.
 */
export function OutcomesCard({ telemetry }: { telemetry: Telemetry | null }) {
  const answers = telemetry?.counters.dns.answers ?? null;

  return (
    <Card
      title="What clients received"
      secondary="answer outcomes, since boot"
      tools={<RefreshCluster registry={refresh} endpoint="telemetry" />}
    >
      {answers === null ? (
        <EmptyState title="Not read yet" />
      ) : (
        <div class="kv">
          <span>SERVFAIL synthesized by FastAdHunter</span>
          <span class="mono num">
            {answers.servfail_synthesized.toLocaleString()}
          </span>
          <span>SERVFAIL relayed from an upstream</span>
          <span class="mono num">
            {answers.servfail_relayed.toLocaleString()}
          </span>
          <span>REFUSED relayed from an upstream</span>
          <span class="mono num">
            {answers.refused_relayed.toLocaleString()}
          </span>
        </div>
      )}
      <p class="note">
        The split matters when something is wrong: synthesized means
        FastAdHunter could not get an answer and said so; relayed means an
        upstream answered with an error and FastAdHunter passed it on unchanged.
        The first points here, the second points at the resolver being used.
        <span class="footnote-line">
          These are process-lifetime totals. The same three figures are
          persisted per interval in{' '}
          <span class="mono">/api/v1/history/perf</span>, so &ldquo;how many
          clients saw an error during that outage&rdquo; survives a restart —
          this page reads the live counters, not that history.
        </span>
      </p>
    </Card>
  );
}
