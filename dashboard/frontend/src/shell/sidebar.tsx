import type { ComponentChildren } from 'preact';
import { Link } from '../router/link';
import {
  SECTION_LABELS,
  navigableRoutes,
  type Route,
  type Section,
} from '../router/routes';
import { Icon } from './icon';

const ICONS: Record<string, string> = {
  '/': 'dashboard',
  '/lists': 'lists',
  '/rules': 'rules',
  '/policies': 'policies',
  '/clients': 'clients',
  '/rule-tester': 'rule-tester',
  '/cache': 'cache',
  '/performance': 'performance',
  '/upstreams': 'upstreams',
  '/settings': 'settings',
  '/dev/gallery': 'diagnostics',
};

const SECTION_ORDER: Section[] = [
  'overview',
  'filtering',
  'runtime',
  'system',
];

const DIAGNOSTICS_ROOT = '/diagnostics/health';

function inDiagnostics(path: string): boolean {
  return path.startsWith('/diagnostics/');
}

/**
 * All thirteen entries, four labelled sections, the nested Diagnostics group.
 * The group expands while a diagnostics route is active and is one line
 * otherwise — the state the artboards draw (`Memory`, `LiveFeed`, `MobileNav`
 * expanded; `Main`, `Cache`, `Upstreams`, `Settings` collapsed).
 */
export function Sidebar({
  path,
  open,
  onNavigate,
  footer,
}: {
  path: string;
  open: boolean;
  onNavigate: () => void;
  /** The drawer footer `MobileNav` draws. Below 768 px the top bar keeps only
   *  the burger and the page title, so connection state, version, theme and
   *  sign-out live here — without it a phone can neither switch theme nor sign
   *  out. */
  footer?: ComponentChildren;
}) {
  const routes = navigableRoutes();
  const diagnostics = routes.filter((route) => route.group === 'diagnostics');
  const expanded = inDiagnostics(path);

  return (
    <nav
      class={`sb${open ? ' open' : ''}`}
      aria-label="Sections"
      id="fah-sidebar"
    >
      <div class="brand">
        <Icon name="brand" size={20} />
        <span>FastAdHunter</span>
      </div>

      <div class="sb-nav">
        {SECTION_ORDER.map((section) => (
          <div key={section}>
            <div class="sec">{SECTION_LABELS[section]}</div>
            {routes
              .filter(
                (route) =>
                  route.section === section && route.group === undefined,
              )
              .map((route) => (
                <Item
                  key={route.path}
                  route={route}
                  path={path}
                  onNavigate={onNavigate}
                />
              ))}
            {section === 'system' && diagnostics.length > 0 && (
              <>
                {/* No `aria-current` on the parent: the active child carries
                    `page`, and two current markers in one subtree is a worse
                    answer to "where am I" than one. */}
                <Link
                  href={DIAGNOSTICS_ROOT}
                  class={`it${expanded ? ' on' : ''}`}
                  onClickCapture={onNavigate}
                >
                  <Icon name="diagnostics" />
                  <span>Diagnostics</span>
                </Link>
                {expanded &&
                  diagnostics.map((route) => (
                    <Link
                      key={route.path}
                      href={route.path}
                      class={`sub2${route.path === path ? ' on' : ''}`}
                      {...(route.path === path
                        ? { 'aria-current': 'page' as const }
                        : {})}
                      onClickCapture={onNavigate}
                    >
                      {route.title}
                    </Link>
                  ))}
              </>
            )}
          </div>
        ))}
      </div>

      {footer !== undefined && <div class="sb-foot">{footer}</div>}
    </nav>
  );
}

function Item({
  route,
  path,
  onNavigate,
}: {
  route: Route;
  path: string;
  onNavigate: () => void;
}) {
  const active = route.path === path;
  return (
    <Link
      href={route.path}
      class={`it${active ? ' on' : ''}`}
      {...(active ? { 'aria-current': 'page' as const } : {})}
      onClickCapture={onNavigate}
    >
      <Icon name={ICONS[route.path] ?? 'diagnostics'} />
      <span>{route.title}</span>
    </Link>
  );
}
