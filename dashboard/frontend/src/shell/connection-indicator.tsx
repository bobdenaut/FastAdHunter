import type { IndicatorState } from '../events/types';

const WORDS: Record<IndicatorState, string> = {
  live: 'live',
  'not-needed-here': 'not needed here',
  reconnecting: 'reconnecting',
};

/**
 * Three states, and only three. Subscriptions are route-scoped, so a closed
 * socket is the correct steady state on nine of the thirteen screens and
 * `not needed here` must not read as a fault; only `reconnecting` is styled as
 * a problem. Every state carries its word as well as its colour.
 *
 * A detail line is secondary text inside `reconnecting` — never a fourth state.
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
