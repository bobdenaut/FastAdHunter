import type { ComponentChildren } from 'preact';
import { Link } from '../router/link';
import {
  GROUP_LABELS,
  SECTION_LABELS,
  navigableRoutes,
  type Route,
  type RouteGroup,
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

/**
 * A group's landing route and the prefix that marks a path as inside it, both
 * read off the group's own routes.
 *
 * **No group name is written in this file.** `GROUP_LABELS` forces a label for
 * a new group, but the block that draws one was still keyed on the literal
 * `'diagnostics'` — so a second group would have compiled and then been dropped
 * from the sidebar in silence, which is the failure the label record was added
 * to remove.
 */
function groupNest(members: readonly Route[]): { root: string; prefix: string } {
  const root = members[0]?.path ?? '/';
  return { root, prefix: root.slice(0, root.lastIndexOf('/') + 1) };
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
  // Every nested group there is, in the order its routes are declared.
  const groups = [
    ...new Set(
      routes
        .map((route) => route.group)
        .filter((group): group is RouteGroup => group !== undefined),
    ),
  ].map((group) => ({
    group,
    members: routes.filter((route) => route.group === group),
  }));

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
            {groups
              .filter(({ members }) => members[0]?.section === section)
              .map(({ group, members }) => (
                <Group
                  key={group}
                  group={group}
                  members={members}
                  path={path}
                  onNavigate={onNavigate}
                />
              ))}
          </div>
        ))}
      </div>

      {footer !== undefined && <div class="sb-foot">{footer}</div>}
    </nav>
  );
}

/**
 * One nested group: its parent line, and its children while a path inside it is
 * active. The group key is also the sprite name — `sidebar.test.tsx` asserts
 * that every group has both a label and a symbol, so a new group cannot ship
 * with a blank tile.
 */
function Group({
  group,
  members,
  path,
  onNavigate,
}: {
  group: RouteGroup;
  members: readonly Route[];
  path: string;
  onNavigate: () => void;
}) {
  const { root, prefix } = groupNest(members);
  const expanded = path.startsWith(prefix);

  return (
    <>
      {/* No `aria-current` on the parent: the active child carries `page`, and
          two current markers in one subtree is a worse answer to "where am I"
          than one. */}
      <Link
        href={root}
        class={`it${expanded ? ' on' : ''}`}
        onClickCapture={onNavigate}
      >
        <Icon name={group} />
        <span>{GROUP_LABELS[group]}</span>
      </Link>
      {expanded &&
        members.map((route) => (
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
