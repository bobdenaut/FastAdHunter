// @vitest-environment jsdom
import { render } from 'preact';
import { afterEach, describe, expect, it } from 'vitest';
import { GROUP_LABELS, ROUTES } from '../router/routes';
import { ConnectionIndicator } from './connection-indicator';
import { Sidebar } from './sidebar';
import { TopBar } from './topbar';

let host: HTMLElement | null = null;

function mount(node: preact.ComponentChild): HTMLElement {
  host = document.createElement('div');
  document.body.append(host);
  render(node, host);
  return host;
}

afterEach(() => {
  if (host !== null) {
    render(null, host);
    host.remove();
    host = null;
  }
});

describe('the sidebar', () => {
  it('renders the four labelled sections', () => {
    const el = mount(<Sidebar path="/" open={false} onNavigate={() => {}} />);
    expect([...el.querySelectorAll('.sec')].map((n) => n.textContent)).toEqual([
      'Overview',
      'Filtering',
      'Runtime',
      'System',
    ]);
  });

  it('renders every product screen plus the Diagnostics group head', () => {
    const el = mount(<Sidebar path="/" open={false} onNavigate={() => {}} />);
    const labels = [...el.querySelectorAll('.it')]
      .map((n) => n.textContent)
      // Registered only under `import.meta.env.DEV`, which vitest sets.
      .filter((label) => label !== 'Component gallery');
    expect(labels).toEqual([
      'Dashboard',
      'Lists',
      'Custom Rules',
      'Policies',
      'Clients',
      'Rule Tester',
      'Cache',
      'Performance',
      'Upstreams',
      'Settings',
      'Diagnostics',
    ]);
  });

  it('keeps the Diagnostics group collapsed off a diagnostics route', () => {
    const el = mount(<Sidebar path="/cache" open={false} onNavigate={() => {}} />);
    expect(el.querySelectorAll('.sub2')).toHaveLength(0);
  });

  it('expands it on a diagnostics route, as Memory and LiveFeed draw it', () => {
    const el = mount(
      <Sidebar path="/diagnostics/memory" open={false} onNavigate={() => {}} />,
    );
    expect([...el.querySelectorAll('.sub2')].map((n) => n.textContent)).toEqual([
      'Health',
      'Memory',
      'Live Feed',
    ]);
    expect(el.querySelector('.sub2.on')?.textContent).toBe('Memory');
  });

  it('marks the active item for assistive technology, not only by colour', () => {
    const el = mount(<Sidebar path="/lists" open={false} onNavigate={() => {}} />);
    const active = el.querySelector('[aria-current="page"]');
    expect(active?.textContent).toBe('Lists');
    expect(active?.classList.contains('on')).toBe(true);
  });

  it('draws every group in the route table, each under its own routes’ section', () => {
    // The block used to be keyed on the literal `'diagnostics'`, so a second
    // group would have compiled — `GROUP_LABELS` forces a label — and then been
    // dropped from the sidebar in silence. Nothing here names a group.
    const groups = new Set(
      ROUTES.map((route) => route.group).filter((group) => group !== undefined),
    );
    for (const group of groups) {
      const members = ROUTES.filter((route) => route.group === group);
      const el = mount(
        <Sidebar
          path={members[0]?.path ?? '/'}
          open={false}
          onNavigate={() => {}}
        />,
      );
      const labels = [...el.querySelectorAll('.it span')].map(
        (node) => node.textContent,
      );
      expect(labels, group).toContain(GROUP_LABELS[group]);
      expect(
        [...el.querySelectorAll('.sub2')].map((node) => node.textContent),
        group,
      ).toEqual(members.map((route) => route.title));
      render(null, el);
      el.remove();
      host = null;
    }
  });

  it('names a group from GROUP_LABELS, which is what the top bar prefixes with', () => {
    // One source for both. The top bar takes `GROUP_LABELS[route.group]` from
    // the shell, so a label that differed here would be the same screen calling
    // itself two things.
    const el = mount(
      <Sidebar path="/diagnostics/memory" open={false} onNavigate={() => {}} />,
    );
    const group = [...el.querySelectorAll('.it span')].find(
      (node) => node.textContent === GROUP_LABELS.diagnostics,
    );
    expect(group).toBeDefined();
    const top = mount(
      <TopBar
        title="Memory"
        group={GROUP_LABELS.diagnostics}
        version={null}
        indicator="not-needed-here"
        detail={null}
        onToggleDrawer={() => {}}
        onToggleTheme={() => {}}
        onSignOut={() => {}}
      />,
    );
    expect(top.querySelector('.nav-title')?.textContent).toBe(
      `${GROUP_LABELS.diagnostics} · Memory`,
    );
    render(null, el);
    el.remove();
  });

  it('carries the drawer state as a class, so the scrim and CSS agree', () => {
    const el = mount(<Sidebar path="/" open onNavigate={() => {}} />);
    expect(el.querySelector('.sb')?.classList.contains('open')).toBe(true);
  });
});

describe('the connection indicator', () => {
  it('carries its word as well as its colour, in all three states', () => {
    for (const [state, word] of [
      ['live', 'live'],
      ['not-needed-here', 'not needed here'],
      ['reconnecting', 'reconnecting'],
    ] as const) {
      const el = mount(<ConnectionIndicator state={state} detail={null} />);
      expect(el.textContent).toContain(word);
      expect(el.querySelector('.conn')?.classList.contains(state)).toBe(true);
      render(null, el);
      el.remove();
    }
    host = null;
  });

  it('announces politely', () => {
    const el = mount(<ConnectionIndicator state="live" detail={null} />);
    expect(el.querySelector('.conn')?.getAttribute('aria-live')).toBe('polite');
  });

  it('shows a detail line inside reconnecting, and adds no fourth state', () => {
    const el = mount(
      <ConnectionIndicator state="reconnecting" detail="server unreachable" />,
    );
    expect(el.textContent).toContain('reconnecting');
    expect(el.textContent).toContain('server unreachable');
    expect(el.querySelector('.conn')?.className).toBe('conn reconnecting');
  });
});

describe('the drawer footer', () => {
  it('is absent when the shell passes nothing', () => {
    const el = mount(<Sidebar path="/" open={false} onNavigate={() => {}} />);
    expect(el.querySelector('.sb-foot')).toBeNull();
  });

  it('carries what the phone top bar drops, so a phone can still act', () => {
    const el = mount(
      <Sidebar
        path="/"
        open
        onNavigate={() => {}}
        footer={
          <>
            <ConnectionIndicator state="live" detail={null} />
            <span>v0.2.20</span>
            <div class="sb-foot-actions">
              <button type="button">Theme</button>
              <button type="button">Sign out</button>
            </div>
          </>
        }
      />,
    );
    const foot = el.querySelector('.sb-foot');
    expect(foot).not.toBeNull();
    expect(foot?.textContent).toContain('live');
    expect(foot?.textContent).toContain('v0.2.20');
    expect([...(foot?.querySelectorAll('button') ?? [])].map((n) => n.textContent))
      .toEqual(['Theme', 'Sign out']);
  });
});
