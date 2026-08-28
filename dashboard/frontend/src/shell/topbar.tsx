import type { IndicatorState } from '../events/types';
import { ConnectionIndicator } from './connection-indicator';
import { Icon } from './icon';

export function TopBar({
  title,
  group,
  version,
  indicator,
  detail,
  onToggleDrawer,
  onToggleTheme,
  onSignOut,
}: {
  title: string;
  /** The parent section, when the route is nested. */
  group?: string | undefined;
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
      {/* A nested route names its parent: `Diagnostics · Memory`, as the
          artboards draw it. Three routes are nested and all three are under
          Diagnostics, so the group is the whole of the prefix. */}
      <div class="nav-title">{group === undefined ? title : `${group} · ${title}`}</div>
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
