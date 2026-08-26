import type { Route } from '../router/routes';

/**
 * Every sidebar entry resolves to a page. Until the task that owns a screen
 * lands, that page says so — no fake figures, and no route that opens a socket
 * or polls an endpoint it cannot render (phase CLAUDE.md rule 1).
 */
export function NotYetBuilt({ route }: { route: Route }) {
  return (
    <div class="empty-state">
      <p class="empty-state-title">{route.title} is not built yet</p>
      <p class="note">
        This screen lands with its own task. Nothing is drawn here because
        nothing behind it is being read.
      </p>
    </div>
  );
}

export default NotYetBuilt;
