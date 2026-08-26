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

export function navigate(
  path: string,
  options: { replace?: boolean; keepScroll?: boolean } = {},
): void {
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

function onPopState(): void {
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
