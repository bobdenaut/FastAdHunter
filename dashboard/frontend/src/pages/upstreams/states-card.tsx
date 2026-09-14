import { Card } from '../../components/card';
import { StatusPill } from '../../components/status-pill';

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
 * What the three pills mean. `adaptive` is the only strategy the engine
 * accepts, so the legend describes its states and nothing else.
 */
export function StatesCard() {
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
        The page names the strategy in force above, so the reading is never
        ambiguous.
      </p>
    </Card>
  );
}
