import type { Telemetry } from '../../api/types';
import { Card } from '../../components/card';
import { EmptyState } from '../../components/empty-state';

/**
 * Where load was shed and where a request was refused.
 *
 * `events_dropped` is **one number covering both pipelines** because they share
 * a single bounded channel — splitting it would invent a distinction the engine
 * does not make. Non-zero means the event stream shed load, not that queries
 * were dropped.
 *
 * No refresh cluster: `/telemetry` already has one on the card above, and a
 * second pair of controls for one endpoint would show two ages for one reading.
 */
export function BackpressureCard({ telemetry }: { telemetry: Telemetry | null }) {
  const counters = telemetry?.counters ?? null;

  return (
    <Card title="Backpressure and refusals">
      {counters === null ? (
        <EmptyState title="Not read yet" />
      ) : (
        <div class="kv">
          <span>events dropped — shed, both pipelines</span>
          <span class="mono num">{counters.events_dropped.toLocaleString()}</span>
          <span>HTTP requests refused — unusable Host</span>
          <span class="mono num">
            {counters.http.refused_claim.toLocaleString()}
          </span>
          <span>HTTP requests refused — egress policy</span>
          <span class="mono num">
            {counters.http.refused_destination.toLocaleString()}
          </span>
          <span>SWR refreshes dropped</span>
          <span class="mono num">{counters.swr.dropped.toLocaleString()}</span>
          <span>SWR refreshes failed</span>
          <span class="mono num">{counters.swr.failed.toLocaleString()}</span>
        </div>
      )}
      <p class="note">
        Events dropped is one number covering both pipelines — they share a
        single bounded channel, so splitting it would invent a distinction the
        engine does not make. Non-zero means the event stream shed load, not
        that queries were dropped.
        <span class="footnote-line">
          Both stop a request before any upstream contact, for different
          reasons, so they are counted apart. An unusable{' '}
          <span class="mono">Host</span> is the request line itself — missing,
          duplicated, or a bare IP the proxy will not serve. An egress-policy
          refusal is the destination the name <em>resolved</em> to falling
          outside the allowed set. Neither says <em>who</em> or <em>why</em>: a
          missing or duplicated <span class="mono">Host</span> is usually a
          broken client, a bare IP usually a probe, and both land on the same
          figure. Refusals log at <span class="mono">debug</span>, so these
          counters are the only standing record that a request was refused at
          all — the reason is in the log line.
        </span>
      </p>
    </Card>
  );
}
