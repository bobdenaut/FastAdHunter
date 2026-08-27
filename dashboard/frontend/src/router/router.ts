/**
 * ~40 lines over `history.pushState` and `popstate`. The route-scoped lifecycle
 * is the invariant this task is measured against, and a library's mount/unmount
 * timing would have to be understood exactly as well as an own one — so the
 * dependency would buy markup sugar and no lifecycle certainty (D1).
 */

type Listener = (path: string) => void;

const listeners = new Set<Listener>();
let installed = false;

export function currentPath(): string {
  return normalize(window.location.pathname);
}

/** Trailing slashes are stripped so `/lists/` and `/lists` are one route. */
export function normalize(path: string): string {
  if (path.length > 1 && path.endsWith('/')) return path.slice(0, -1);
  return path === '' ? '/' : path;
}

function announce(path: string): void {
  for (const listener of listeners) listener(path);
}

let navigationBlocks = 0;
let heldPath: string | null = null;

/**
 * Held while a mutation that recompiles the ruleset is in flight. Those
 * requests are **never aborted**: Axum drops a handler future when its
 * connection closes, so a cancel mid-compile can land between persist and swap
 * and leave the config ahead of the live matcher, with the client unable to
 * tell whether the write happened. Unmount is what would abort them, so
 * unmount is what this prevents.
 *
 * Counted rather than boolean, and the release is idempotent, so an unmount
 * racing a response cannot leave the application permanently un-navigable.
 */
export function blockNavigation(): () => void {
  navigationBlocks += 1;
  heldPath = currentPath();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    navigationBlocks -= 1;
    if (navigationBlocks === 0) heldPath = null;
  };
}

export function navigationBlocked(): boolean {
  return navigationBlocks > 0;
}

export function navigate(
  path: string,
  options: { replace?: boolean; keepScroll?: boolean } = {},
): void {
  if (navigationBlocks > 0) return;
  const next = normalize(path);
  if (next === currentPath() && !options.replace) return;
  if (options.replace) {
    window.history.replaceState(null, '', next);
  } else {
    window.history.pushState(null, '', next);
  }
  if (options.keepScroll !== true) window.scrollTo(0, 0);
  announce(next);
}

export function subscribeRoute(listener: Listener): () => void {
  listeners.add(listener);
  if (!installed) {
    window.addEventListener('popstate', onPopState);
    installed = true;
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && installed) {
      window.removeEventListener('popstate', onPopState);
      installed = false;
    }
  };
}

/**
 * Back/Forward is in-app navigation too: while a recompiling mutation holds
 * the block, a popstate must not unmount the page out from under a request
 * that is never aborted. The entry the browser moved to is replaced with the
 * held path and nothing is announced, so the page stays mounted and the modal
 * stays up.
 */
function onPopState(): void {
  if (navigationBlocks > 0) {
    window.history.pushState(null, '', heldPath ?? currentPath());
    return;
  }
  announce(currentPath());
}

/** A plain left-click is the only one intercepted: a modifier or a non-primary
 *  button means the reader asked the browser for a new tab or window. */
export function isPlainLeftClick(event: MouseEvent): boolean {
  return (
    event.button === 0 &&
    !event.metaKey &&
    !event.ctrlKey &&
    !event.shiftKey &&
    !event.altKey &&
    !event.defaultPrevented
  );
}
