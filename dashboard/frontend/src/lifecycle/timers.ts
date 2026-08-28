import { AGE_TICK_MS } from '../constants';

/**
 * The only module that may schedule anything. A stray timer outliving its page
 * is the most likely way the route-scoped invariant rots, and confining every
 * scheduler to one file is what makes the grep in `timers.test.ts` able to say
 * so. That grep is test enforcement, not an architectural guarantee — JS has no
 * way to prevent an equivalent scheduler without a lint rule, and claiming
 * otherwise would oversell it.
 */

export type Cancel = () => void;

export function nowMs(): number {
  return Date.now();
}

/** One-shot. The returned cancel is idempotent. */
export function after(delayMs: number, run: () => void): Cancel {
  const handle = setTimeout(run, delayMs);
  return () => clearTimeout(handle);
}

/** Repeating. The returned cancel is idempotent. */
export function every(periodMs: number, run: () => void): Cancel {
  const handle = setInterval(run, periodMs);
  return () => clearInterval(handle);
}

type TickListener = () => void;

const ageListeners = new Set<TickListener>();
let ageTimer: Cancel | null = null;

/**
 * One shared 30 s ticker for every mounted `data-age`, refcounted: it starts on
 * the first subscriber and the last unsubscribe stops it. It lives here rather
 * than in the refresh registry, which owns requests — an age display is a
 * presentation concern and coupling it to the request lifecycle would buy
 * nothing but a shorter rule.
 */
export function subscribeAgeTick(listener: TickListener): Cancel {
  ageListeners.add(listener);
  if (ageTimer === null) {
    ageTimer = every(AGE_TICK_MS, () => {
      for (const tick of ageListeners) tick();
    });
  }
  return () => {
    ageListeners.delete(listener);
    if (ageListeners.size === 0 && ageTimer !== null) {
      ageTimer();
      ageTimer = null;
    }
  };
}

export function ageTickerRunning(): boolean {
  return ageTimer !== null;
}

/**
 * One animation frame, and the third scheduler this module owns.
 *
 * It is what the Live Feed coalesces its rendering on: a burst of query events
 * costs one render per frame rather than one per event, and **a hidden document
 * does not fire frames at all** — so rendering stops while the page is hidden
 * by construction rather than by a visibility handler of the feed's own. The
 * ring behind it is bounded and keeps absorbing meanwhile.
 */
export function onNextFrame(run: () => void): Cancel {
  const handle = requestAnimationFrame(run);
  return () => cancelAnimationFrame(handle);
}
