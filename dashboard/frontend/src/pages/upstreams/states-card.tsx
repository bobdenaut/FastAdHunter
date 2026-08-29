import { Card } from '../../components/card';
import { StatusPill } from '../../components/status-pill';
import type { UpstreamMode } from '../../derive';

const STATES = [
  {
    state: 'healthy' as const,
    text: 'Answering normally. Preferred for new queries.',
  },
  {
    state: 'penalized' as const,
    text: 'Failed enough to be stepped back. Each round lengthens the penalty. Still queried when nothing else is available — being penalized is not being removed.',
  },
  {
    state: 'probing' as const,
    text: 'Under test after a penalty. Successful probes return it to healthy.',
  },
];

/**
 * What the three pills mean — and, under `fallback`, that there are no pills to
 * explain. The body is swapped rather than annotated: a legend for states that
 * do not exist is the same mistake as rendering their zeros.
 */
export function StatesCard({ mode }: { mode: UpstreamMode }) {
  if (mode === 'fallback') {
    return (
      <Card title="What the states mean">
        <p class="note">
          Under the <span class="mono">fallback</span> strategy these states do
          not apply — no health state exists to report, which is why this page
          shows none. Every row publishes{' '}
          <span class="mono">state: healthy</span> and zeros for penalties,
          probes and penalized seconds whatever the endpoint is doing.
        </p>
        <p class="note">
          <span class="mono">degraded</span> there simply means every endpoint
          carries a non-zero consecutive-failure count. A secondary is only
          attempted when the primary fails, so a streak on one can be hours old.
        </p>
      </Card>
    );
  }

  return (
    <Card title="What the states mean">
      <div class="state-legend">
        {STATES.map((entry) => (
          <div key={entry.state}>
            <StatusPill status={entry.state} />
            <span class="note">{entry.text}</span>
          </div>
        ))}
      </div>
      <p class="note state-legend-foot">
        Under the <span class="mono">fallback</span> strategy these states do
        not apply — <span class="mono">degraded</span> there simply means every
        endpoint carries a non-zero consecutive-failure count. The page names
        the strategy in force so the reading is never ambiguous.
      </p>
    </Card>
  );
}
