import { useEffect, useRef } from 'preact/hooks';

export const FOCUSABLE =
  'a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])';

/**
 * The container counts when it is itself focusable. `querySelectorAll` only
 * walks descendants, so a modal whose sole focusable element is its own root —
 * the busy modal, which has nothing to press — came back empty: focus stayed
 * on `body` and the first `Tab` was the only thing that ever moved it in.
 */
export function focusableWithin(node: HTMLElement | null): HTMLElement[] {
  if (node === null) return [];
  const inside = [...node.querySelectorAll<HTMLElement>(FOCUSABLE)];
  return node.matches(FOCUSABLE) ? [node, ...inside] : inside;
}

/**
 * Focus moves in on activation, cycles inside while active, and returns to
 * whatever invoked it on release; `Escape` asks the owner to close. One
 * implementation for the confirm dialog and the phone drawer — two copies of
 * the Tab arithmetic is two places for it to be subtly wrong.
 *
 * `resolve` and `onEscape` are read through a ref so the effect keys on
 * `active` alone: a caller passing an inline arrow must not rebind the listener
 * and re-steal focus on every render.
 */
export function useFocusTrap(
  active: boolean,
  resolve: () => HTMLElement | null,
  onEscape: () => void,
): void {
  const latest = useRef({ resolve, onEscape });
  latest.current = { resolve, onEscape };

  useEffect(() => {
    if (!active) return;
    const invoker = document.activeElement as HTMLElement | null;
    focusableWithin(latest.current.resolve())[0]?.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        latest.current.onEscape();
        return;
      }
      if (event.key !== 'Tab') return;
      const items = focusableWithin(latest.current.resolve());
      const first = items[0];
      const last = items[items.length - 1];
      if (first === undefined || last === undefined) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      invoker?.focus();
    };
  }, [active]);
}
