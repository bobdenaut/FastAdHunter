/**
 * **Documentation-sourced constants, not figures this page measured.**
 *
 * Both come from PERFORMANCE.md §Budgets and are drawn as **markers** — a
 * dashed rule with a caption — never as a wall, a clamp or a red zone. Nothing
 * enforces them at runtime: the container runs `memory-high=unlimited`, so a
 * crossing is a budget breach to investigate against a pre-change build, not a
 * failure the engine will act on. A chart that implies a hard limit invites
 * someone to set one, and `memory-high=200M` has already OOM-killed this
 * household's live resolver once.
 */

/** Steady-state target. Decimal MB, as PERFORMANCE.md writes it. */
export const STEADY_STATE_BUDGET = 128_000_000;

/** The ceiling the same section names. Also decimal MB. */
export const CEILING_BUDGET = 256_000_000;

/**
 * A budget as the page prints it: decimal MB, because that is the unit
 * PERFORMANCE.md states them in and the whole page exists partly to keep MB and
 * MiB apart. One formatter so a budget change moves every figure and every
 * caption at once — the literals it replaced had already drifted from the
 * constants beside them once.
 */
export function budgetLabel(bytes: number): string {
  return `${String(bytes / 1_000_000)} MB`;
}

export const STEADY_STATE_LABEL = `${budgetLabel(STEADY_STATE_BUDGET)} steady-state budget`;
export const CEILING_LABEL = `${budgetLabel(CEILING_BUDGET)} hard-ceiling budget`;


const MIB = 1024 * 1024;

/**
 * Where the RSS line stops being ink.
 *
 * **100 MiB is not a round number chosen for looking tidy.** It sits just above
 * the highest value the sampled series has been observed to reach, so a reading
 * above it is territory this process has not been in — while still well under
 * the steady-state budget. A watch point set at, say, half the budget would be
 * on during ordinary operation, and a threshold that is always tripped is not a
 * threshold.
 */
export const WATCH_THRESHOLD = 100 * MIB;

/**
 * How far RSS has to fall back before the state drops again.
 *
 * Without it a reading hovering on a threshold alternates state between
 * samples, and a line that changes colour every 60 s reads as an event when
 * nothing happened. The band is deliberately wider than sampling noise and far
 * narrower than the gap between the two thresholds.
 */
export const HYSTERESIS = 3 * MIB;

/**
 * Ink below the watch point, amber-shaded between, red above the budget.
 *
 * **`previous` is what makes this hysteretic, and it only ever loosens on the
 * way down.** A reading rises into a state at the threshold itself and falls
 * out of it `HYSTERESIS` below — so there is exactly one threshold to
 * document, the 100 MiB watch point and the 128 MB budget, and the band under
 * each is the width the state has to fall through before it is given up.
 *
 * `null` is the first reading and takes the plain thresholds, both of them.
 * The earlier form tested `previous === 'normal'` for the watch point, which
 * left `null` on the **falling** threshold: a first reading was `watch` from
 * 97 MiB while the chart drew and captioned its rule at 100 MiB — two
 * thresholds for one documented value, on the page whose subject is a budget.
 */
export type RssState = 'normal' | 'watch' | 'over';

export function rssState(bytes: number, previous: RssState | null): RssState {
  const over =
    previous === 'over' ? STEADY_STATE_BUDGET - HYSTERESIS : STEADY_STATE_BUDGET;
  if (bytes >= over) return 'over';
  const watch =
    previous === 'watch' || previous === 'over'
      ? WATCH_THRESHOLD - HYSTERESIS
      : WATCH_THRESHOLD;
  return bytes >= watch ? 'watch' : 'normal';
}

/**
 * The state of every reading in a sampled series, hysteresis carried forward.
 *
 * **This is where the 3 MiB band is actually spent.** The RSS trend line is
 * the element the sketch describes as hysteretic, and it is the only one that
 * sees successive readings — one per 60 s sample — so it is the only place a
 * sticky threshold can mean anything. The KPI card reads the tail of the same
 * walk, so the card and the line can never disagree about a reading.
 *
 * A missing sample takes no state and does not reset the one being carried: a
 * gap is a row nobody wrote, not evidence that RSS fell.
 */
export function rssStates(
  values: ArrayLike<number | null | undefined>,
): (RssState | null)[] {
  const states: (RssState | null)[] = [];
  let previous: RssState | null = null;
  for (let index = 0; index < values.length; index += 1) {
    const value = values[index];
    if (value === null || value === undefined) {
      states.push(null);
      continue;
    }
    previous = rssState(value, previous);
    states.push(previous);
  }
  return states;
}

/**
 * The state the newest reading is in, with the window's history behind it.
 *
 * `normal` when the series is empty or all gaps — the plain-threshold answer
 * for a value nothing precedes, which is what `rssState(_, null)` gives too.
 */
export function latestRssState(
  values: ArrayLike<number | null | undefined>,
): RssState {
  const states = rssStates(values);
  for (let index = states.length - 1; index >= 0; index -= 1) {
    const state = states[index];
    if (state !== null && state !== undefined) return state;
  }
  return 'normal';
}
