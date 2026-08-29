/**
 * Visibility is an input, never a second authority. This module observes
 * `visibilitychange` and reports it. It owns no timer, opens nothing and closes
 * nothing: the socket manager owns the connection and the refresh registry owns
 * which endpoints are polled, and each interprets the flag itself.
 *
 * With two actors able to open and close the socket, a reconnect can race the
 * hidden-close grace timer and either tear down a connection a route just asked
 * for, or leave one open that nothing wants. One owner per resource makes that
 * race unrepresentable rather than merely unlikely.
 */

type Listener = (hidden: boolean) => void;

const listeners = new Set<Listener>();
let installed = false;

export function isHidden(): boolean {
  return typeof document !== 'undefined' && document.visibilityState === 'hidden';
}

function onVisibilityChange(): void {
  const hidden = isHidden();
  for (const listener of listeners) listener(hidden);
}

export function subscribeVisibility(listener: Listener): () => void {
  listeners.add(listener);
  if (!installed && typeof document !== 'undefined') {
    document.addEventListener('visibilitychange', onVisibilityChange);
    installed = true;
  }
  // The current state, not only the next change: a tab opened or
  // session-restored in the background would otherwise poll its route and hold
  // a socket until the first `visibilitychange` — which for a tab that is never
  // brought forward is never.
  listener(isHidden());
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && installed) {
      document.removeEventListener('visibilitychange', onVisibilityChange);
      installed = false;
    }
  };
}
