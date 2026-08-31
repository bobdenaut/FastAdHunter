import type { Telemetry, UpstreamState } from '../../api/types';
import { NO_RTT, rttLabel } from '../../charts/format';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { RefreshCluster } from '../../components/refresh-cluster';
import { upstreamBar } from '../../derive';
import type { RefreshRegistry } from '../../refresh/registry';

/**
 * The unit is kept when the value is missing — `— ms`, never a bare `—`. Every
 * row ends in the same shape, so the four endpoints stay readable as a column
 * rather than as sentences of differing length. The Upstreams page prints the
 * bare dash instead, because there the figure sits under a `p50` label that
 * already carries the unit.
 *
 * Lifetime, like every figure on this card: `/telemetry` is cumulative since
 * process start, so this is how far away the endpoint typically is, not how it
 * is answering right now. The per-interval view is the Upstreams page's chart.
 */
function rttSummary(seconds: number | undefined): string {
  const label = rttLabel(seconds);
  return label === NO_RTT ? `${NO_RTT} ms` : label;
}

const STATE_TONE: Record<UpstreamState, string> = {
  healthy: 'var(--state-live)',
  penalized: 'var(--state-problem)',
  probing: 'var(--pill-special-fg)',
};

/**
 * Endpoint health, **never a share of traffic**: FastAdHunter deliberately does
 * not carry per-query upstream attribution, so a traffic-share chart would be
 * invented data.
 *
 * The encoding is D4/A and `derive.ts` owns the arithmetic — the bar carries
 * workload, the overlay carries the failure rate within that bar, and `state`
 * is carried by the dot alone. The `N attempts · M failures` text is always
 * present, which is what keeps a sub-pixel failure band from losing the figure.
 */
export function UpstreamHealth({
  telemetry,
  strategy,
  registry,
  className,
}: {
  telemetry: Telemetry | null;
  strategy: string | null;
  registry: RefreshRegistry;
  className?: string;
}) {
  const upstreams = telemetry?.upstreams ?? [];
  const max = upstreams.reduce((top, row) => Math.max(top, row.attempts), 0);

  return (
    <Card
      title="Upstream health"
      tools={
        <RefreshCluster
          registry={registry}
          endpoint="telemetry"
          {...(strategy === null ? {} : { secondary: `strategy: ${strategy}` })}
        />
      }
      bodyClass="upstreams"
      className={className}
    >
      {upstreams.length === 0 ? (
        <EmptyState title="No upstream figures yet" />
      ) : (
        upstreams.map((upstream) => {
          const bar = upstreamBar(upstream.attempts, upstream.failures, max);
          // The median, not the tail and not the mean: `p99` is bucket-quantized
          // and jumps a whole bucket at a time, and the mean is cumulative over
          // the process lifetime and stops moving within hours. `p50` is the one
          // that stays comparable between rows, which is what a summary card
          // showing four endpoints side by side is read for.
          const p50 = rttSummary(upstream.rtt?.p50);
          return (
            <div class="upstream" key={upstream.address}>
              <div class="upstream-head">
                <span class="mono">
                  <span
                    class="dot"
                    style={{ background: STATE_TONE[upstream.state] }}
                  />{' '}
                  {upstream.address}{' '}
                  <span class="note">
                    {upstream.protocol}
                    {upstream.state === 'healthy'
                      ? ''
                      : ` · ${upstream.state}`}
                  </span>
                </span>
                {/* Three cells rather than one sentence: every endpoint's row
                    is its own flex line, so a single string right-aligned only
                    lines its last character up and `14,172 attempts` pushed
                    `0 attempts` out of column. Fixed `ch` tracks in the mono
                    face are the same on every row, which is what makes the
                    four read as a column. */}
                <span class="mono upstream-figures">
                  <span>{upstream.attempts.toLocaleString()} attempts</span>
                  <span>{upstream.failures.toLocaleString()} failures</span>
                  {/* Beside the counters, not on a row of its own: `state` and
                      `failures` are what this card is read for, and a fourth
                      line per endpoint doubles its height. */}
                  <span>{p50}</span>
                </span>
              </div>
              <div class="bar">
                <span
                  class="upstream-work"
                  style={{ width: `${String(bar.width * 100)}%` }}
                >
                  {/* No minimum width: a band under one pixel disappears, and
                      the figure survives in the text above. */}
                  <i style={{ width: `${String(bar.overlay * 100)}%` }} />
                </span>
              </div>
            </div>
          );
        })
      )}
      <p class="note">
        Endpoint health, not share of traffic — FastAdHunter deliberately does
        not carry per-query upstream attribution, so a traffic-share chart would
        be invented data.
      </p>
    </Card>
  );
}
