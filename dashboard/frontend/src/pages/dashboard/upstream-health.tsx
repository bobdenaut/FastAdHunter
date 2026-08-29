import type { Telemetry, UpstreamState } from '../../api/types';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';
import { RefreshCluster } from '../../components/refresh-cluster';
import { upstreamBar } from '../../derive';
import type { RefreshRegistry } from '../../refresh/registry';

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
                <span class="mono upstream-figures">
                  {upstream.attempts.toLocaleString()} attempts ·{' '}
                  {upstream.failures.toLocaleString()} failures
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
