import type { IndicatorState } from '../events/types';

const WORDS: Record<IndicatorState, string> = {
  live: 'live',
  'not-needed-here': 'not needed here',
  reconnecting: 'reconnecting',
  'api-unreachable': 'API not answering',
};

/**
 * Subscriptions are route-scoped, so a closed socket is the correct steady
 * state on nine of the thirteen screens. `not needed here` answered a question
 * nobody asks in the place the operator reads health, so on those screens the
 * shell reports the API instead: `live` once it has answered,
 * `API not answering` when a read did not reach it. Only `reconnecting` and
 * `api-unreachable` are styled as a problem; every state carries its word as
 * well as its colour.
 *
 * A detail line is secondary text inside `reconnecting` — never a state.
 */
export function ConnectionIndicator({
  state,
  detail,
}: {
  state: IndicatorState;
  detail: string | null;
}) {
  return (
    <span class={`conn ${state}`} aria-live="polite">
      <span class="dot" />
      <span>{WORDS[state]}</span>
      {detail !== null && <span class="conn-detail">· {detail}</span>}
    </span>
  );
}
