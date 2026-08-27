// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setUnauthorizedHandler } from '../api/core';
import type { ListsResponse, PoliciesResponse, Stats } from '../api/types';
import { navigate, navigationBlocked } from '../router/router';
import type { Route } from '../router/routes';
import Policies from './policies';

/**
 * The acceptance criterion is "policy operations that recompile are confirmed;
 * those that do not are not, and the difference is stated in the UI". Both
 * halves are asserted here, along with R2's consequence: a recompiling
 * mutation blocks in a modal, blocks navigation and cannot be joined by a
 * second one.
 */

const ROUTE: Route = {
  path: '/policies',
  title: 'Policies',
  section: 'filtering',
  events: [],
  endpoints: [],
  built: true,
  ownsHeader: true,
  load: null,
};

const POLICIES: PoliciesResponse = {
  timezone: 'EET-2EEST,M3.5.0/3,M10.5.0/4',
  active_assignments: 2,
  items: [
    {
      id: 'kids',
      name: 'Kids',
      lists: ['oisd-basic', 'hagezi-pro'],
      blocking_mode: null,
      assignments: [
        { client: '192.168.10.50', days: 'mon-fri', start: '21:00', end: '07:00' },
        { client: '192.168.10.22', days: 'sat-sun', start: '20:00', end: '09:00' },
      ],
    },
    {
      id: 'guest',
      name: 'Guest Wi-Fi',
      lists: null,
      blocking_mode: null,
      assignments: [{ client: '192.168.20.0/24' }],
    },
  ],
};

const STATS = {
  window: '24h',
  queries_total: 0,
  blocked_total: 0,
  blocked_percent: 0,
  cache_hit_percent: 0,
  top_blocked_domains: [],
  top_queried_domains: [],
  top_clients: [],
  buckets: [],
  policies: [
    { policy: 'default', queries: 176204, blocked: 21890 },
    { policy: 'kids', queries: 8118, blocked: 3204 },
  ],
} satisfies Stats;

const LISTS: ListsResponse = {
  compiled_rules: 752585,
  duplicates_removed: 12,
  items: [
    {
      id: 'oisd-basic',
      url: 'https://example.invalid/oisd',
      format: 'adblock',
      enabled: true,
      refresh_hours: 24,
      last_refresh: null,
      last_status: 'never',
      rules_total: 1,
      rules_active_dns: 1,
      rules_active_url: 0,
      rules_inactive: 0,
      parse_errors: 0,
    },
    {
      id: 'hagezi-pro',
      url: 'https://example.invalid/hagezi',
      format: 'adblock',
      enabled: true,
      refresh_hours: 24,
      last_refresh: null,
      last_status: 'never',
      rules_total: 1,
      rules_active_dns: 1,
      rules_active_url: 0,
      rules_inactive: 0,
      parse_errors: 0,
    },
  ],
};

function respond(status: number, body?: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: () => null },
    json: async () => {
      if (body === undefined) throw new Error('no body');
      return body;
    },
  } as unknown as Response;
}

const fetchMock = vi.fn();
let host: HTMLElement | null = null;

function base(path: string): Response {
  if (path === '/api/v1/policies') return respond(200, POLICIES);
  if (path === '/api/v1/stats') return respond(200, STATS);
  if (path === '/api/v1/lists') return respond(200, LISTS);
  throw new Error(`unexpected ${path}`);
}

async function flush(): Promise<void> {
  await act(async () => {
    for (let turn = 0; turn < 8; turn += 1) await Promise.resolve();
  });
}

async function mount(): Promise<HTMLElement> {
  host = document.createElement('div');
  document.body.append(host);
  await act(async () => {
    render(<Policies route={ROUTE} />, host as HTMLElement);
  });
  await flush();
  return host;
}

function calls(): Array<[string, string]> {
  return fetchMock.mock.calls.map((call) => [
    (call[1] as RequestInit | undefined)?.method ?? 'GET',
    call[0] as string,
  ]);
}

function byText(dom: HTMLElement, label: string): HTMLButtonElement {
  const found = [...dom.querySelectorAll('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (found === undefined) throw new Error(`no button: ${label}`);
  return found;
}

/** The topmost dialog: a confirmation stacks over the policy form. */
function confirmDialog(dom: HTMLElement): HTMLElement {
  const dialogs = [...dom.querySelectorAll<HTMLElement>('.dialog')];
  const last = dialogs[dialogs.length - 1];
  if (last === undefined) throw new Error('no dialog');
  return last;
}

function confirmButton(dom: HTMLElement): HTMLButtonElement {
  return confirmDialog(dom).querySelector(
    '.dialog-actions .btn:not(.g)',
  ) as HTMLButtonElement;
}

async function click(node: Element | null | undefined): Promise<void> {
  if (node === null || node === undefined) throw new Error('nothing to click');
  await act(async () => {
    (node as HTMLElement).click();
  });
  await flush();
}

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  setUnauthorizedHandler(null);
  fetchMock.mockImplementation((path: string) => Promise.resolve(base(path)));
});

afterEach(() => {
  if (host !== null) {
    act(() => {
      render(null, host as HTMLElement);
    });
    host.remove();
    host = null;
  }
  vi.unstubAllGlobals();
});

describe('the read path', () => {
  it('reads three endpoints on mount and nothing else', async () => {
    await mount();
    expect(calls()).toEqual([
      ['GET', '/api/v1/policies'],
      ['GET', '/api/v1/stats'],
      ['GET', '/api/v1/lists'],
    ]);
  });

  it('renders the summary figures the plan lists, and no others', async () => {
    const dom = await mount();
    const figures = [...dom.querySelectorAll('.figure-row > div')].map(
      (node) => [
        node.querySelector('.figure')?.textContent,
        node.querySelector('.note')?.textContent,
      ],
    );
    expect(figures).toEqual([
      ['3 / 16', 'policies · ceiling'],
      ['2', 'assignments in force right now'],
      ['3', 'assignments configured'],
      ['EET-2EEST', 'schedule timezone'],
    ]);
    // The count is assignments in force after name expansion, so it is never
    // phrased as "of N configured" — the two are genuinely different numbers.
    expect(dom.textContent).not.toContain('of 3 configured');
  });

  it('keeps the full POSIX timezone as the title', async () => {
    const dom = await mount();
    const cell = dom.querySelector('[title]');
    expect(cell?.getAttribute('title')).toBe(POLICIES.timezone);
  });

  it('draws a synthetic Default card with no Edit and no Delete', async () => {
    const dom = await mount();
    const cards = [...dom.querySelectorAll('.policy-card')];
    const defaultCard = cards[0] as HTMLElement;
    expect(defaultCard.textContent).toContain('default');
    expect(defaultCard.querySelector('.pill')?.textContent).toBe('reserved');
    expect(defaultCard.querySelector('.lchip')?.textContent).toBe(
      'every enabled list',
    );
    expect(defaultCard.querySelectorAll('button')).toHaveLength(0);
  });

  it('matches traffic by id, and draws an empty track for a policy with none', async () => {
    const dom = await mount();
    const fills = [...dom.querySelectorAll<HTMLElement>('.policy-traffic-fill')];
    // default 21890/176204, kids 3204/8118, guest has no /stats row at all.
    expect(fills.map((node) => node.style.width)).toEqual([
      '12.42310049715103%',
      '39.467849223946786%',
      '0%',
    ]);
  });

  it('reports the slots left and states the rule count from `/lists`', async () => {
    const dom = await mount();
    expect(dom.querySelector('.policy-slot')?.textContent).toContain(
      '13 policy slots left',
    );
    expect(dom.querySelector('.cost-body')?.textContent).toContain('752,585 rules');
  });

  it('renders assignment rows with no in-force claim of their own', async () => {
    const dom = await mount();
    const rows = [...dom.querySelectorAll('.assignment')].map(
      (row) => row.textContent,
    );
    expect(rows).toEqual([
      '192.168.10.50mon–fri · 21:00 → 07:00',
      '192.168.10.22sat–sun · 20:00 → 09:00',
      '192.168.20.0/24no schedule — always',
    ]);
    expect(dom.textContent).not.toContain('ACTIVE NOW');
  });
});

describe('the recompile boundary', () => {
  it('renames without a confirmation, a modal or a navigation block', async () => {
    const dom = await mount();
    await click(byText(dom, 'Edit'));
    const name = dom.querySelector('#policy-name') as HTMLInputElement;
    await act(async () => {
      name.value = 'Children';
      name.dispatchEvent(new Event('input', { bubbles: true }));
    });
    let settle: ((value: Response) => void) | null = null;
    fetchMock.mockImplementationOnce(
      () =>
        new Promise<Response>((resolve) => {
          settle = resolve;
        }),
    );
    await click(byText(dom, 'Save changes'));
    expect(dom.querySelector('[aria-busy="true"]')).toBeNull();
    expect(navigationBlocked()).toBe(false);
    await act(async () => {
      settle?.(respond(200, POLICIES.items[0]));
    });
    await flush();
    const body = JSON.parse(
      (fetchMock.mock.calls[3]?.[1] as RequestInit).body as string,
    ) as unknown;
    // Only the changed field: an unchanged subset sent back would still not
    // recompile, but it makes an edit unreadable in a request log.
    expect(body).toEqual({ name: 'Children' });
    expect(calls().slice(3)).toEqual([
      ['PATCH', '/api/v1/policies/kids'],
      ['GET', '/api/v1/policies'],
    ]);
  });

  it('sends only `assignments` when only assignments changed', async () => {
    const dom = await mount();
    await click(byText(dom, 'Edit'));
    await click(dom.querySelector('.assignment-editor .iconbtn'));
    await click(byText(dom, 'Save changes'));
    const body = JSON.parse(
      (fetchMock.mock.calls[3]?.[1] as RequestInit).body as string,
    ) as { assignments?: unknown[]; lists?: unknown };
    expect(body.assignments).toHaveLength(1);
    expect('lists' in body).toBe(false);
    expect(dom.querySelector('[aria-busy="true"]')).toBeNull();
  });

  it('confirms a `lists` change, then blocks until the compile answers', async () => {
    const dom = await mount();
    await click(byText(dom, 'Edit'));
    const everyList = dom.querySelector(
      'input[name="policy-lists"]',
    ) as HTMLInputElement;
    await act(async () => {
      everyList.checked = true;
      everyList.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await click(byText(dom, 'Save changes'));

    // Confirmation first, and it names the cost.
    expect(confirmDialog(dom).querySelector('h2')?.textContent).toBe(
      'Change which lists this policy holds?',
    );
    expect(confirmDialog(dom).textContent).toContain(
      'seconds of CPU on the router',
    );
    expect(calls()).toHaveLength(3);

    let settle: ((value: Response) => void) | null = null;
    fetchMock.mockImplementationOnce(
      () =>
        new Promise<Response>((resolve) => {
          settle = resolve;
        }),
    );
    await click(confirmButton(dom));

    expect(dom.querySelector('[aria-busy="true"]')).not.toBeNull();
    expect(navigationBlocked()).toBe(true);
    const before = window.location.pathname;
    navigate('/clients');
    expect(window.location.pathname).toBe(before);

    const body = JSON.parse(
      (fetchMock.mock.calls[3]?.[1] as RequestInit).body as string,
    ) as unknown;
    // The double option: `null` clears the subset back to every enabled list,
    // and the key must be present for the server to see the difference.
    expect(body).toEqual({ lists: null });

    await act(async () => {
      settle?.(respond(200, POLICIES.items[0]));
    });
    await flush();
    expect(dom.querySelector('[aria-busy="true"]')).toBeNull();
    expect(navigationBlocked()).toBe(false);
  });

  it('confirms a delete and names what goes with it', async () => {
    const dom = await mount();
    await click(byText(dom, 'Delete'));
    expect(confirmDialog(dom).querySelector('h2')?.textContent).toBe(
      'Delete kids?',
    );
    expect(confirmDialog(dom).textContent).toContain(
      '2 assignments go with it',
    );
    fetchMock.mockImplementationOnce(() => Promise.resolve(respond(204)));
    await click(confirmButton(dom));
    expect(calls().slice(3)).toEqual([
      ['DELETE', '/api/v1/policies/kids'],
      ['GET', '/api/v1/policies'],
    ]);
  });
});

describe('the form-enforced limits', () => {
  it('refuses `default` as an id and reports the alphabet', async () => {
    const dom = await mount();
    await click(byText(dom, 'New policy'));
    const id = dom.querySelector('#policy-id') as HTMLInputElement;
    await act(async () => {
      id.value = 'default';
      id.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(dom.querySelector('.field-error')?.textContent).toContain(
      'implicit policy',
    );
    expect(byText(dom, 'Create policy').disabled).toBe(true);

    await act(async () => {
      id.value = 'Kids Room';
      id.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(dom.querySelector('.field-error')?.textContent).toContain(
      'lowercase letters',
    );
  });

  it('renders the API 409 in the dialog and keeps what was typed', async () => {
    const dom = await mount();
    await click(byText(dom, 'New policy'));
    const id = dom.querySelector('#policy-id') as HTMLInputElement;
    await act(async () => {
      id.value = 'kids';
      id.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await click(byText(dom, 'Create policy'));
    fetchMock.mockImplementationOnce(() =>
      Promise.resolve(
        respond(409, {
          error: { code: 'conflict', message: 'policy kids already exists' },
        }),
      ),
    );
    await click(confirmButton(dom));
    expect(dom.querySelector('.error-state-message')?.textContent).toBe(
      'policy kids already exists',
    );
    expect((dom.querySelector('#policy-id') as HTMLInputElement).value).toBe(
      'kids',
    );
  });

  it('disables New policy at the ceiling and says why', async () => {
    fetchMock.mockImplementation((path: string) => {
      if (path === '/api/v1/policies') {
        return Promise.resolve(
          respond(200, {
            ...POLICIES,
            items: Array.from({ length: 15 }, (_, index) => ({
              id: `p${String(index)}`,
              name: `P${String(index)}`,
              lists: null,
              blocking_mode: null,
              assignments: [],
            })),
          }),
        );
      }
      return Promise.resolve(base(path));
    });
    const dom = await mount();
    expect(dom.querySelector('.policy-slot')?.textContent).toContain(
      '0 policy slots left',
    );
    for (const button of [...dom.querySelectorAll('button')].filter(
      (candidate) => candidate.textContent?.trim() === 'New policy',
    )) {
      expect(button.disabled).toBe(true);
    }
    expect(dom.querySelector('.policy-slot')?.textContent).toContain(
      'The ceiling is 16',
    );
  });
});

describe('the empty list subset (F3)', () => {
  it('renders `lists: []` as blocking nothing, never as every enabled list', async () => {
    fetchMock.mockImplementation((path: string) => {
      if (path === '/api/v1/policies') {
        return Promise.resolve(
          respond(200, {
            ...POLICIES,
            items: [
              {
                id: 'bare',
                name: 'Bare',
                lists: [],
                blocking_mode: null,
                assignments: [],
              },
            ],
          }),
        );
      }
      return Promise.resolve(base(path));
    });
    const dom = await mount();
    const card = [...dom.querySelectorAll('.policy-card')].find((candidate) =>
      candidate.textContent?.includes('bare'),
    );
    expect(card?.textContent).toContain('no lists — blocks nothing');
    expect(card?.querySelector('.policy-lists')?.textContent).not.toContain(
      'every enabled list',
    );
  });

  it('refuses to submit an empty subset from the dialog', async () => {
    fetchMock.mockImplementation((path: string) =>
      Promise.resolve(base(path)),
    );
    const dom = await mount();
    await act(async () => {
      byText(dom, 'Edit').click();
    });
    await flush();
    const dialog = confirmDialog(dom);
    const onlyThese = [...dialog.querySelectorAll('label')].find((label) =>
      label.textContent?.includes('only these'),
    );
    const radio = onlyThese?.querySelector('input') as HTMLInputElement;
    await act(async () => {
      radio.click();
    });
    await flush();
    for (const box of dialog.querySelectorAll<HTMLInputElement>(
      '.subset-picker input[type="checkbox"]',
    )) {
      if (box.checked) {
        await act(async () => {
          box.click();
        });
      }
    }
    await flush();
    expect(dialog.textContent).toContain('pick at least one list');
    const save = [...dialog.querySelectorAll('button')].find(
      (candidate) => candidate.textContent?.trim() === 'Save changes',
    ) as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    const writes = calls().filter(([method]) => method !== 'GET');
    expect(writes).toEqual([]);
  });
});

describe('mutations racing the route (F12)', () => {
  it('does not re-read after unmount when a live mutation settles late', async () => {
    const dom = await mount();
    await click(byText(dom, 'Edit'));
    const name = dom.querySelector('#policy-name') as HTMLInputElement;
    await act(async () => {
      name.value = 'Children';
      name.dispatchEvent(new Event('input', { bubbles: true }));
    });
    let settle: ((value: Response) => void) | null = null;
    fetchMock.mockImplementationOnce(
      () =>
        new Promise<Response>((resolve) => {
          settle = resolve;
        }),
    );
    await click(byText(dom, 'Save changes'));
    const before = calls().length;

    await act(async () => {
      render(null, host as HTMLElement);
    });
    await act(async () => {
      settle?.(respond(200, POLICIES.items[0]));
    });
    await flush();

    expect(calls().length).toBe(before);
  });
});
