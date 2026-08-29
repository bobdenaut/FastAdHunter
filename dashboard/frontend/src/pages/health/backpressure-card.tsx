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
          <span>HTTP requests refused by egress policy</span>
          <span class="mono num">{counters.http.refused.toLocaleString()}</span>
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
          Refused is the egress policy stopping a request before any upstream
          contact — an unusable <span class="mono">Host</span>, or a destination
          outside the allowed set. Refusals log at{' '}
          <span class="mono">debug</span>, so this counter is the only standing
          signal that a LAN client is probing.
        </span>
      </p>
    </Card>
  );
}
