import { Link } from '../router/link';

/**
 * The unknown-URL screen. Every declared route is built, so the only way here
 * is an address the route table does not know — a typo, a stale bookmark, or a
 * dev-only path in a production build. It says that, rather than the
 * not-yet-built story the placeholder told while unbuilt routes still existed.
 */
export function NotFound() {
  return (
    <div class="empty-state">
      <p class="empty-state-title">No screen at this address</p>
      <p class="note">
        The address in the bar does not match any page of this dashboard.{' '}
        <Link href="/">Go to the Dashboard</Link>.
      </p>
    </div>
  );
}

export default NotFound;
