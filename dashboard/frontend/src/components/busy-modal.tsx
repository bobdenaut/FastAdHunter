import type { ComponentChildren } from 'preact';
import { useRef } from 'preact/hooks';
import { useFocusTrap } from './focus-trap';

/**
 * A blocking wait with **no cancel and no faked progress** — `p5-06`'s
 * Refresh-all treatment, and the only correct one for a request the server
 * holds open for a full ruleset rebuild.
 *
 * It has no dismiss because dismissing is what must not happen: the four
 * recompiling mutations are never aborted, so this modal is what stops an
 * unmount while one is in flight, alongside the router's navigation block. The
 * focus trap therefore gets a no-op escape handler rather than a close.
 *
 * The bar is indeterminate on purpose. The API reports nothing about compile
 * progress, so a moving percentage would be a number with no field behind it.
 */
export function BusyModal({
  title,
  children,
}: {
  title: string;
  children?: ComponentChildren;
}) {
  const dialog = useRef<HTMLDivElement>(null);

  useFocusTrap(true, () => dialog.current, () => undefined);

  return (
    <div class="dialog-scrim">
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-busy="true"
        aria-label={title}
        // The one focusable thing inside, because there is nothing to press:
        // without it the trap has nowhere to put focus and Tab walks straight
        // out of a modal whose whole job is to be inescapable.
        tabIndex={0}
        ref={dialog}
      >
        <h2>{title}</h2>
        {children !== undefined && <div class="note">{children}</div>}
        <div class="progress-indeterminate" />
      </div>
    </div>
  );
}
