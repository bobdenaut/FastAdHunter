import type { Upstream, UpstreamRtt } from '../../api/types';
import { millisLabel } from '../../charts/format';
import { StatusPill } from '../../components/status-pill';
import { failureRunShares, type UpstreamMode } from '../../derive';

/** What a cell prints when the engine served no `rtt` block, or served one no
 *  attempt has answered into yet. Distinct from `0.00 ms`, which would claim a
 *  measurement. */
const NO_RTT = '—';

/** The word that goes where `v4` or `v6` would, and the reason it is not one.
 *  Nothing resolves a DoH URL's hostname to fill the field in — it is not a
 *  lookup — so `unknown` is the honest reading and `null` is never printed. */
export const FAMILY_UNKNOWN = 'family unknown';

const FAMILY_TITLE =
  'The host is a domain name resolved at connect time — nothing resolves it to fill the field in.';

/** The histogram's four buckets, `[len 1, len 2, len 3, len >= 4]`. */
const RUN_LENGTHS = 'runs of length 1,2,3,4+';

/**
 * One configured endpoint. The index is the row's position in configured
 * order, which is the same identity a query reports as its answering endpoint
 * (CONTEXT.md §Answering Endpoint) — it is read off the array, never invented.
 *
 * **The strategy gates the health half of the row.** Under `fallback` every row
 * publishes `state: healthy`, `penalty_round: 0` and zeros for penalties,
 * penalized seconds, probes and probe successes, which API.md states means *no
 * health state exists to report* — so those cells and the state pill are
 * omitted rather than rendered as good news.
 */
export function EndpointRow({
  index,
  upstream,
  mode,
}: {
  index: number;
  upstream: Upstream;
  mode: UpstreamMode;
}) {
  const health = mode !== 'fallback';
  const penalized = health && upstream.state === 'penalized';
  const runs = failureRunShares(upstream.failure_runs);
  const rtt = upstream.rtt;

  return (
    <div
      class={penalized ? 'ep is-penalized' : 'ep'}
      data-endpoint={String(index)}
    >
      <div class="ep-identity">
        <div class="ep-head">
          <span class="ep-index mono">{index}</span>
          <span class="ep-address mono">{upstream.address}</span>
        </div>
        <div class="ep-state">
          {health && <StatusPill status={upstream.state} />}
          <span class="note mono">
            {upstream.protocol} ·{' '}
            {upstream.family === null ? (
              <span title={FAMILY_TITLE}>{FAMILY_UNKNOWN}</span>
            ) : (
              upstream.family
            )}
            {/* `penalty_round` is the last penalty applied and is never cleared
                by recovery, so a healthy endpoint still publishes the round it
                reached. Printing it there would imply an active state the field
                does not carry. */}
            {penalized && ` · round ${String(upstream.penalty_round)}`}
          </span>
        </div>
      </div>

      <div class="ep-counters">
        <Cell label="attempts" value={upstream.attempts.toLocaleString()} />
        <Cell label="failures" value={upstream.failures.toLocaleString()} />
        <Cell
          label="consecutive"
          value={upstream.consecutive_failures.toLocaleString()}
          // The live figure, and the only one on the row that is: it resets on
          // a success, where the totals beside it say what has happened ever.
          tone={upstream.consecutive_failures === 0 ? 'good' : 'bad'}
        />
        <Cell
          label="TLS handshakes"
          value={upstream.tls_handshakes.toLocaleString()}
        />
        {health && (
          <>
            <Cell label="penalties" value={upstream.penalties.toLocaleString()} />
            <Cell
              label="penalized for"
              value={`${upstream.penalized_seconds_total.toLocaleString()} s`}
            />
            <Cell label="probes" value={upstream.probes.toLocaleString()} />
            <Cell
              label="probe successes"
              value={upstream.probe_successes.toLocaleString()}
            />
          </>
        )}
      </div>

      <div class="ep-rtt">
        <div class="note">round trip, answered attempts only</div>
        <div class="ep-rtt-cells">
          <Cell label="p50" value={rttLabel(rtt?.p50)} />
          <Cell label="p99" value={rttLabel(rtt?.p99)} />
          <Cell label="mean" value={meanLabel(rtt)} />
          <Cell
            label="answers timed"
            value={rtt === undefined ? NO_RTT : rtt.count.toLocaleString()}
          />
        </div>
      </div>

      <div class="ep-runs">
        <div class="note">failure-run histogram</div>
        <div class="runs">
          {runs.map((share, bucket) => (
            // The buckets are fixed and positional, so the index is the
            // identity here rather than a stand-in for one. Each bar keeps its
            // own track, so an endpoint that has closed no runs draws four
            // empty ones instead of nothing at all.
            <div key={bucket} class="run-track">
              <div
                class="run-bar"
                style={{ height: `${String(share * 100)}%` }}
              />
            </div>
          ))}
        </div>
        <div class="note mono">
          {upstream.failure_runs.join(' · ')}{' '}
          <span class="run-legend">{RUN_LENGTHS}</span>
        </div>
      </div>
    </div>
  );
}

/** Seconds to a millisecond label. An exact `0` is "no attempt reached this
 *  percentile", the same reading the Performance page's tiles take, so it
 *  prints as nothing rather than as an impossibly fast endpoint. */
function rttLabel(seconds: number | undefined): string {
  if (seconds === undefined || seconds === 0) return NO_RTT;
  return `${millisLabel(seconds * 1000)} ms`;
}

/** The one exact figure in the group: `sum / count`, both cumulative. A
 *  lifetime mean flattens within hours, which is why the percentiles beside it
 *  are per-interval on the chart — this is here to catch the case the buckets
 *  cannot show, an endpoint whose every answer sits past the top bucket. */
function meanLabel(rtt: UpstreamRtt | undefined): string {
  if (rtt === undefined || rtt.count === 0) return NO_RTT;
  return `${millisLabel((rtt.sum_seconds / rtt.count) * 1000)} ms`;
}

function Cell({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone?: 'good' | 'bad';
}) {
  return (
    <div>
      <div class="k note">{label}</div>
      <div class={tone === undefined ? 'mono' : `mono ${tone}`}>{value}</div>
    </div>
  );
}
