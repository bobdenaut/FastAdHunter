import type { ComponentType } from 'preact';
import { useEffect, useLayoutEffect, useState } from 'preact/hooks';
import { getHealth } from '../api/health';
import { apiReach, subscribeApiReach, type ApiReach } from '../api/core';
import type { IndicatorState } from '../events/types';
import { subscribeVisibility } from '../lifecycle/visibility';
import { currentPath, navigate, subscribeRoute } from '../router/router';
import {
  LOGIN_ROUTE,
  routeFor,
  type PageProps,
  type Route,
} from '../router/routes';
import { logout } from '../api/auth';
import { routeLifecycle, socket } from '../services';
import { LOGIN_PATH } from '../session/session';
import { useSessionGuard } from '../session/guard';
import { toggleTheme } from '../theme/theme';
import { NotYetBuilt } from '../pages/not-yet-built';
import { ErrorState } from '../components/error-state';
import { useFocusTrap } from '../components/focus-trap';
import { ConnectionIndicator } from './connection-indicator';
import { ContentHeader } from './content-header';
import { Sidebar } from './sidebar';
import { TopBar } from './topbar';
import { Icon } from './icon';

/** The UI and the API deploy as one artifact, so a chunk that 404s means the
 *  page outlived the build it came from. Reloading is the whole remedy. */
const CHUNK_FAILED = new Error(
  'This screen could not be loaded. The dashboard was probably updated while this page was open — reload it.',
);

const NOT_FOUND: Route = {
  path: '*',
  title: 'Not found',
  section: 'overview',
  events: [],
  endpoints: [],
  built: false,
  load: null,
};

export function Shell() {
  const [path, setPath] = useState(currentPath);
  const [Page, setPage] = useState<ComponentType<PageProps> | null>(null);
  const [indicator, setIndicator] = useState<IndicatorState>('not-needed-here');
  const [reach, setReach] = useState<ApiReach>(apiReach);
  const [detail, setDetail] = useState<string | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [drawer, setDrawer] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);

  useEffect(() => subscribeRoute(setPath), []);
  useSessionGuard();

  useEffect(
    () =>
      socket.subscribeState(() => {
        setIndicator(socket.indicator());
        setDetail(socket.detail());
      }),
    [],
  );

  // No clock: this moves when a page reads, which is the only thing that
  // happens on a route with no subscription.
  useEffect(() => subscribeApiReach(setReach), []);

  // An input, never an actor: the flag is handed to the owners, which decide.
  useEffect(() => subscribeVisibility((hidden) => routeLifecycle.setSuspended(hidden)), []);

  // One shot per shell mount, never polled. `/health`'s periodic reads belong
  // to the shared refresh and happen only while a mounted page asks for them.
  useEffect(() => {
    const controller = new AbortController();
    getHealth(controller.signal)
      .then((health) => setVersion(health.version))
      .catch(() => setVersion(null));
    return () => controller.abort();
  }, []);

  const route = routeFor(path) ?? NOT_FOUND;

  // A route with a subscription reports its socket; one without reports the
  // API, because "no socket here" is not an answer to the question the pill
  // occupies the space for. `unknown` — nothing read yet — keeps the old word.
  const shown: IndicatorState =
    indicator !== 'not-needed-here'
      ? indicator
      : reach === 'reachable'
        ? 'live'
        : reach === 'unreachable'
          ? 'api-unreachable'
          : 'not-needed-here';

  const signOut = () => {
    void logout()
      .catch(() => undefined)
      .then(() => navigate(LOGIN_PATH, { replace: true }));
  };

  // The transition is the only caller of acquire/release, and it runs before
  // the incoming page renders.
  useLayoutEffect(() => {
    routeLifecycle.enter(route);
    setIndicator(socket.indicator());
    setDetail(socket.detail());
  }, [route]);

  useEffect(() => {
    let live = true;
    const load = route.load;
    setLoadFailed(false);
    if (load === null) {
      setPage(null);
      return;
    }
    setPage(null);
    void load()
      .then((module) => {
        if (live) setPage(() => module.default);
      })
      .catch(() => {
        // A hashed chunk that 404s — an open tab after a redeploy, since the
        // UI and the API version as one artifact. Without this the route sits
        // on the boot placeholder for ever.
        if (live) setLoadFailed(true);
      });
    return () => {
      live = false;
    };
  }, [route]);

  useEffect(() => setDrawer(false), [path]);

  // The drawer is a modal overlay on the phone: focus moves in, cycles inside,
  // `Escape` closes it, and focus returns to the burger. Below 768 px the
  // closed drawer is also `visibility: hidden`, so its thirteen links leave the
  // tab order and the accessibility tree rather than sitting off-screen.
  useFocusTrap(
    drawer,
    () => document.getElementById('fah-sidebar'),
    () => setDrawer(false),
  );

  if (path === LOGIN_PATH) {
    if (loadFailed) return <ErrorState error={CHUNK_FAILED} />;
    return Page === null ? <div class="boot" /> : <Page route={LOGIN_ROUTE} />;
  }

  const body =
    route.load === null ? (
      <NotYetBuilt route={route} />
    ) : loadFailed ? (
      <ErrorState error={CHUNK_FAILED} />
    ) : Page === null ? (
      <div class="boot" />
    ) : (
      <Page route={route} />
    );

  return (
    <div class="app">
      <Sidebar
        path={path}
        open={drawer}
        onNavigate={() => setDrawer(false)}
        footer={
          <>
            <ConnectionIndicator state={shown} detail={detail} />
            {version !== null && <span>v{version}</span>}
            <div class="sb-foot-actions">
              <button type="button" onClick={() => toggleTheme()}>
                Theme
              </button>
              <button type="button" onClick={signOut}>
                Sign out
              </button>
            </div>
          </>
        }
      />
      {drawer && (
        <>
          <button
            type="button"
            class="scrim open"
            aria-label="Close navigation"
            onClick={() => setDrawer(false)}
          />
          <button
            type="button"
            class="drawer-close"
            aria-label="Close navigation"
            onClick={() => setDrawer(false)}
          >
            <Icon name="close" size={20} />
          </button>
        </>
      )}
      <div class="main">
        <TopBar
          title={route.title}
          version={version}
          indicator={shown}
          detail={detail}
          onToggleDrawer={() => setDrawer((open) => !open)}
          onToggleTheme={() => toggleTheme()}
          onSignOut={signOut}
        />
        {/* A route that owns its header renders the header *and* the `.wrap`
            itself, because the two are siblings — `.hd` sits outside the
            padded content column. Until its chunk resolves the shell renders
            neither, so an entry cannot flash a title the page is about to
            replace with its own wording. */}
        {route.ownsHeader === true && Page !== null && !loadFailed ? (
          body
        ) : (
          <>
            {route.ownsHeader !== true && <ContentHeader title={route.title} />}
            <main class="wrap">{body}</main>
          </>
        )}
      </div>
    </div>
  );
}
