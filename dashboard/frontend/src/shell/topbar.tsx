import type { IndicatorState } from '../events/types';
import { ConnectionIndicator } from './connection-indicator';
import { Icon } from './icon';

export function TopBar({
  title,
  version,
  indicator,
  detail,
  onToggleDrawer,
  onToggleTheme,
  onSignOut,
}: {
  title: string;
  version: string | null;
  indicator: IndicatorState;
  detail: string | null;
  onToggleDrawer: () => void;
  onToggleTheme: () => void;
  onSignOut: () => void;
}) {
  return (
    <header class="nav">
      <button
        type="button"
        class="burger"
        aria-label="Open navigation"
        aria-controls="fah-sidebar"
        onClick={onToggleDrawer}
      >
        <Icon name="menu" size={21} />
      </button>
      <div class="nav-title">{title}</div>
      <div class="navr">
        <ConnectionIndicator state={indicator} detail={detail} />
        {/* `version` comes from GET /health, fetched once per shell mount and
            never polled. */}
        {version !== null && <span class="navr-version">v{version}</span>}
        <button
          type="button"
          class="navr-theme"
          onClick={onToggleTheme}
          title="Switch theme"
        >
          <Icon name="theme" size={14} />
          theme
        </button>
        <button type="button" class="navr-signout" onClick={onSignOut}>
          <Icon name="sign-out" size={14} />
          sign out
        </button>
      </div>
    </header>
  );
}
