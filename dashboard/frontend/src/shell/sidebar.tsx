import type { ComponentChildren } from 'preact';
import { useState } from 'preact/hooks';
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
  '/live-feed': 'live-feed',
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
 * The prefix that marks a path as inside a group, read off the group's own
 * routes. There is no landing route to derive: the group head opens the group
 * rather than going anywhere.
 *
 * **No group name is written in this file.** `GROUP_LABELS` forces a label for
 * a new group, but the block that draws one was still keyed on the literal
 * `'diagnostics'` — so a second group would have compiled and then been dropped
 * from the sidebar in silence, which is the failure the label record was added
 * to remove.
 */
function groupPrefix(members: readonly Route[]): string {
  const root = members[0]?.path ?? '/';
  return root.slice(0, root.lastIndexOf('/') + 1);
}

/**
 * All thirteen entries, four labelled sections, the nested Diagnostics group.
 * The group expands while a diagnostics route is active and is one line
 * otherwise — the state the artboards draw (`Memory`, `MobileNav` expanded;
 * `Main`, `Cache`, `Upstreams`, `Settings` collapsed). The Live Feed left the
 * group for Overview, so its artboard no longer shows the group expanded.
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
 * One nested group: a disclosure head and the children it reveals. The group
 * key is also the sprite name — `sidebar.test.tsx` asserts that every group has
 * both a label and a symbol, so a new group cannot ship with a blank tile.
 *
 * **The head is a button, not a link.** It has no landing route of its own:
 * pointing it at the first child made "show me what is in here" load a screen
 * nobody asked for, and on a phone it closed the drawer on the way.
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
  const inside = path.startsWith(groupPrefix(members));

  /**
   * The path governs across a navigation, the button between them: entering
   * the group opens it, leaving closes it back to the one line the artboards
   * draw, and off-route it is whatever it was last set to.
   *
   * The state is therefore *which path a toggle was made on*, not a second
   * copy of "open". Seeding `useState(inside)` would not do — the sidebar
   * outlives every navigation, so arriving at a child by Back or by typed URL
   * would leave the active row hidden inside a collapsed group. Nor would
   * syncing that copy in an effect: effects flush after the click that reads
   * them, so a mount whose effect was still pending swallowed the first press.
   * Held this way there is nothing to keep in step — any navigation makes the
   * toggle stale and the path speaks again.
   */
  const [toggledOn, setToggledOn] = useState<string | null>(null);
  const expanded = toggledOn === path ? !inside : inside;

  return (
    <>
      {/* `on` tracks the active group, not the disclosure: an expanded group
          whose screens are not the one on show must not read as where you are.
          No `aria-current` either — the active child carries `page`, and two
          current markers in one subtree is a worse answer to "where am I" than
          one. */}
      <button
        type="button"
        class={`it it-group${inside ? ' on' : ''}`}
        aria-expanded={expanded}
        onClick={() =>
          setToggledOn((current) => (current === path ? null : path))
        }
      >
        <Icon name={group} />
        <span>{GROUP_LABELS[group]}</span>
      </button>
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
