import type uPlot from 'uplot';

/**
 * **The only module in the application that imports uPlot at runtime**, and it
 * does so dynamically. One static import anywhere collapses the chunk split
 * silently and puts the chart library on the login path.
 *
 * It exists as a module of its own because two callers need the library and
 * neither may import it: `components/chart.tsx` constructs the plot, and
 * `charts/stacked-bars.ts` needs `uPlot.paths.bars` from inside a draw
 * callback. Reaching it through the live instance does not work — uPlot returns
 * a plain object, so `u.constructor` is `Object` and the statics are not on it.
 */

let resolved: typeof uPlot | null = null;
let pending: Promise<typeof uPlot> | null = null;

/** Imported once and shared: a second chart on a page costs no second fetch. */
export function loadUPlot(): Promise<typeof uPlot> {
  pending ??= import('uplot').then((module) => {
    resolved = module.default;
    return module.default;
  });
  return pending;
}

/** Non-null once `loadUPlot` has resolved, which any draw callback is past by
 *  definition. Callers that could run earlier must handle the `null`. */
export function loadedUPlot(): typeof uPlot | null {
  return resolved;
}
