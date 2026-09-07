// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setUnauthorizedHandler } from '../api/core';
import type { Client, PoliciesResponse } from '../api/types';
import type { Route } from '../router/routes';
import Clients from './clients';

/**
 * The acceptance criterion is "loading Clients issues one request for the
 * list, asserted by counting requests, not by inspection" — plus D1's second
 * one-shot, and the standing prohibition on `GET /clients/{ip}/policy`. An
 * N+1 here is invisible on a dev box with three clients, so it is counted.
 */

const ROUTE: Route = {
  path: '/clients',
  title: 'Clients',
  section: 'filtering',
  events: [],
  endpoints: [],
  built: true,
  ownsHeader: true,
  load: null,
};

function client(overrides: Partial<Client> & Pick<Client, 'ip'>): Client {
  return {
    name: null,
    first_seen: '2026-08-27T08:00:00Z',
    last_seen: new Date().toISOString(),
    queries_24h: 100,
    blocked_24h: 10,
    policy: 'default',
    ...overrides,
  };
}

const CLIENTS: Client[] = [
  client({ ip: '192.168.10.15', name: 'liviu-phone', queries_24h: 30122, blocked_24h: 3020 }),
  client({
    ip: '192.168.10.50',
    name: 'tv',
    policy: 'kids',
    assignment_source: 'direct',
    queries_24h: 18204,
    blocked_24h: 6902,
  }),
  client({
    ip: '192.168.10.22',
    policy: 'default',
    assignment_source: 'direct',
    queries_24h: 9118,
    blocked_24h: 1204,
  }),
  client({ ip: '192.168.20.11', policy: 'guest', queries_24h: 1204, blocked_24h: 388 }),
  client({ ip: '192.168.10.7', name: 'printer', queries_24h: 0, blocked_24h: 0 }),
];

const POLICIES: PoliciesResponse = {
  timezone: 'EET-2EEST,M3.5.0/3,M10.5.0/4',
  active_assignments: 2,
  items: [
    {
      id: 'kids',
      name: 'Kids',
      lists: null,
      blocking_mode: null,
      assignments: [
        { client: '192.168.10.50', days: 'mon-fri', start: '21:00', end: '07:00' },
        { client: '192.168.10.22', days: 'sat-sun' },
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

/** What the page reads on mount: it opens on IPv4. */
const CLIENTS_V4 = '/api/v1/clients?family=v4';

function route(path: string, init?: RequestInit): Response {
  const method = init?.method ?? 'GET';
  const [pathname, query] = path.split('?');
  if (method === 'GET' && pathname === '/api/v1/clients') {
    // Every fixture address is IPv4, so the API narrows `v6` to nothing.
    const family = new URLSearchParams(query).get('family');
    return respond(200, { items: family === 'v6' ? [] : CLIENTS });
  }
  if (method === 'GET' && path === '/api/v1/policies') return respond(200, POLICIES);
  if (method === 'PUT' && path.endsWith('/policy')) {
    return respond(200, { ip: '192.168.10.50', policy: 'kids' });
  }
  if (method === 'DELETE' && path.endsWith('/policy')) return respond(204);
  if (method === 'PUT') return respond(200, CLIENTS[0]);
  throw new Error(`unexpected ${method} ${path}`);
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
    render(<Clients route={ROUTE} />, host as HTMLElement);
  });
  await flush();
  return host;
}

function calls(): Array<[string, string]> {
  return fetchMock.mock.calls.map((call) => [
    ((call[1] as RequestInit | undefined)?.method ?? 'GET'),
    call[0] as string,
  ]);
}

function rows(dom: HTMLElement): HTMLElement[] {
  return [...dom.querySelectorAll<HTMLElement>('.client-row')];
}

/** A row by the address it is about, so a test never encodes the sort order. */
function rowFor(dom: HTMLElement, ip: string): HTMLElement | undefined {
  return rows(dom).find(
    (row) => row.querySelector('.c-ip')?.textContent?.trim() === ip,
  );
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
  fetchMock.mockImplementation((path: string, init?: RequestInit) =>
    Promise.resolve(route(path, init)),
  );
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

describe('what loading the page costs', () => {
  it('issues exactly two requests and no per-row policy read', async () => {
    const dom = await mount();
    expect(rows(dom)).toHaveLength(CLIENTS.length);
    expect(calls()).toEqual([
      ['GET', CLIENTS_V4],
      ['GET', '/api/v1/policies'],
    ]);
    expect(
      calls().filter(([method, path]) => method === 'GET' && /\/policy$/.test(path)),
    ).toEqual([]);
  });

  it('filters client-side, without a request', async () => {
    const dom = await mount();
    const search = dom.querySelector('.search-input') as HTMLInputElement;
    await act(async () => {
      search.value = 'tv';
      search.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(rows(dom)).toHaveLength(1);
    expect(rows(dom)[0]?.querySelector('.c-name')?.textContent).toBe('tv');
    expect(calls()).toHaveLength(2);
  });

  it('opens on IPv4 and re-reads with `?family=` when a chip is picked', async () => {
    const dom = await mount();
    const group = dom.querySelector('[aria-label="Filter by address family"]');
    const chip = (label: string) =>
      [...(group?.querySelectorAll('button') ?? [])].find(
        (button) => button.textContent === label,
      );
    expect(chip('IPv4')?.getAttribute('aria-pressed')).toBe('true');

    await click(chip('IPv6'));
    expect(rows(dom)).toHaveLength(0);
    expect(dom.querySelector('.empty-state-title')?.textContent).toBe(
      'No IPv6 client has asked anything yet',
    );
    await click(chip('all'));
    expect(rows(dom)).toHaveLength(CLIENTS.length);
    expect(calls().slice(2)).toEqual([
      ['GET', '/api/v1/clients?family=v6'],
      ['GET', '/api/v1/policies'],
      ['GET', '/api/v1/clients'],
      ['GET', '/api/v1/policies'],
    ]);
  });
});

describe('the policy column', () => {
  it('renders every classification branch the fixture reaches', async () => {
    const dom = await mount();
    const chips = rows(dom).map((row) => ({
      chip: row.querySelector('.pchip')?.textContent,
      dashed: row.querySelector('.pchip')?.classList.contains('inh'),
      note: row.querySelector('.pol-note')?.textContent,
    }));
    // Descending by address, so `192.168.20.11` leads and `192.168.10.7` ends.
    expect(chips).toEqual([
      { chip: 'guest', dashed: true, note: 'via 192.168.20.0/24' },
      {
        chip: 'kids',
        dashed: false,
        note: 'mon–fri · 21:00 → 07:00 · in force',
      },
      {
        chip: 'kids',
        dashed: false,
        note: 'sat–sun · all day · window shut now — default in force',
      },
      {
        chip: 'default',
        dashed: true,
        note: 'inherited · no assignment',
      },
      { chip: 'default', dashed: true, note: 'inherited · no assignment' },
    ]);
  });

  it('renders `unnamed` and a 0 % share without dividing by zero', async () => {
    const dom = await mount();
    expect(rows(dom)[2]?.querySelector('.c-name')?.textContent).toBe('unnamed');
    expect(rows(dom)[2]?.querySelector('.c-name')?.className).toContain(
      'unnamed',
    );
    const printer = rows(dom)[4];
    expect(printer?.querySelector('.c-share-figure')?.textContent).toBe('0 %');
    expect(
      (printer?.querySelector('.c-share-fill') as HTMLElement).style.width,
    ).toBe('0%');
  });
});

describe('the three mutations', () => {
  it('renames without a confirmation and re-reads both responses', async () => {
    const dom = await mount();
    await click(rows(dom)[4]?.querySelector('.iconbtn'));
    await click(
      [...(rows(dom)[4]?.querySelectorAll('button') ?? [])].find(
        (button) => button.textContent === 'Rename',
      ),
    );
    const input = dom.querySelector('.rename .field-input') as HTMLInputElement;
    await act(async () => {
      input.value = 'laserjet';
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    // No dialog anywhere: nothing on this page recompiles.
    expect(dom.querySelector('.dialog')).toBeNull();
    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Save',
      ),
    );
    expect(calls().slice(2)).toEqual([
      ['PUT', '/api/v1/clients/192.168.10.7'],
      ['GET', CLIENTS_V4],
      ['GET', '/api/v1/policies'],
    ]);
  });

  it('assigns a policy with a schedule, and validates before the API does', async () => {
    const dom = await mount();
    // By address, not by index: the table orders descending, so a row's
    // position is a rendering decision this test does not depend on.
    await click(rowFor(dom, '192.168.10.15')?.querySelector('.iconbtn'));
    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Change policy',
      ),
    );
    const select = dom.querySelector('#assign-policy') as HTMLSelectElement;
    await act(async () => {
      select.value = 'kids';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    const days = dom.querySelector('#assign-days') as HTMLInputElement;
    await act(async () => {
      days.value = 'funday';
      days.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(dom.querySelector('.field-error')?.textContent).toContain(
      'unknown day',
    );
    const assign = [...dom.querySelectorAll('button')].find(
      (button) => button.textContent === 'Assign',
    ) as HTMLButtonElement;
    expect(assign.disabled).toBe(true);

    await act(async () => {
      days.value = 'mon-fri';
      days.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const start = dom.querySelector('#assign-start') as HTMLInputElement;
    await act(async () => {
      start.value = '21:00';
      start.dispatchEvent(new Event('input', { bubbles: true }));
    });
    // Half-open windows never close, so the form refuses one.
    expect(dom.querySelector('.field-error')?.textContent).toContain(
      'both start and end',
    );
    const end = dom.querySelector('#assign-end') as HTMLInputElement;
    await act(async () => {
      end.value = '07:00';
      end.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(dom.querySelector('.field-error')).toBeNull();

    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Assign',
      ),
    );
    const body = JSON.parse(
      (fetchMock.mock.calls[2]?.[1] as RequestInit).body as string,
    ) as unknown;
    expect(body).toEqual({
      policy: 'kids',
      days: 'mon-fri',
      start: '21:00',
      end: '07:00',
    });
    expect(calls().slice(2)).toEqual([
      ['PUT', '/api/v1/clients/192.168.10.15/policy'],
      ['GET', CLIENTS_V4],
      ['GET', '/api/v1/policies'],
    ]);
  });

  it('clears the assignment when `default` is chosen', async () => {
    const dom = await mount();
    await click(rows(dom)[1]?.querySelector('.iconbtn'));
    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Change policy',
      ),
    );
    const select = dom.querySelector('#assign-policy') as HTMLSelectElement;
    await act(async () => {
      select.value = 'default';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Clear assignment',
      ),
    );
    expect(calls().slice(2)).toEqual([
      ['DELETE', '/api/v1/clients/192.168.10.50/policy'],
      ['GET', CLIENTS_V4],
      ['GET', '/api/v1/policies'],
    ]);
  });

  it('treats a 404 on the clear as the state the operator asked for', async () => {
    const dom = await mount();
    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      if ((init?.method ?? 'GET') === 'DELETE') {
        return Promise.resolve(
          respond(404, {
            error: { code: 'not_found', message: 'no assignment for 192.168.10.50' },
          }),
        );
      }
      return Promise.resolve(route(path, init));
    });
    await click(rows(dom)[1]?.querySelector('.iconbtn'));
    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Change policy',
      ),
    );
    const select = dom.querySelector('#assign-policy') as HTMLSelectElement;
    await act(async () => {
      select.value = 'default';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Clear assignment',
      ),
    );
    expect(dom.querySelector('.error-state')).toBeNull();
    expect(calls().slice(2).map(([method]) => method)).toEqual([
      'DELETE',
      'GET',
      'GET',
    ]);
  });

  it('reports a 404 on the assign and re-reads, because the copy is stale', async () => {
    const dom = await mount();
    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      if ((init?.method ?? 'GET') === 'PUT') {
        return Promise.resolve(
          respond(404, {
            error: { code: 'not_found', message: 'no such policy: kids' },
          }),
        );
      }
      return Promise.resolve(route(path, init));
    });
    await click(rows(dom)[0]?.querySelector('.iconbtn'));
    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Change policy',
      ),
    );
    const select = dom.querySelector('#assign-policy') as HTMLSelectElement;
    await act(async () => {
      select.value = 'kids';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Assign',
      ),
    );
    expect(dom.querySelector('.error-state-message')?.textContent).toBe(
      'no such policy: kids',
    );
    expect(calls().slice(2).map(([method]) => method)).toEqual([
      'PUT',
      'GET',
      'GET',
    ]);
  });
});

describe('empty states', () => {
  it('says no client has asked yet rather than reporting a failure', async () => {
    fetchMock.mockImplementation((path: string, init?: RequestInit) =>
      Promise.resolve(
        path === CLIENTS_V4
          ? respond(200, { items: [] })
          : route(path, init),
      ),
    );
    const dom = await mount();
    expect(dom.querySelector('.empty-state-title')?.textContent).toBe(
      'No IPv4 client has asked anything yet',
    );
    expect(dom.querySelector('.error-state')).toBeNull();
  });
});

describe('mutations racing the route (F4/F5/F6)', () => {
  it('does not re-read after unmount when a mutation settles late', async () => {
    let settle: ((value: Response) => void) | null = null;
    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      if ((init?.method ?? 'GET') === 'PUT') {
        return new Promise<Response>((resolve) => {
          settle = resolve;
        });
      }
      return Promise.resolve(route(path, init));
    });
    const dom = await mount();
    await click(rows(dom)[4]?.querySelector('.iconbtn'));
    await click(
      [...(rows(dom)[4]?.querySelectorAll('button') ?? [])].find(
        (button) => button.textContent === 'Rename',
      ),
    );
    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Save',
      ),
    );
    const before = calls().length;

    await act(async () => {
      render(null, host as HTMLElement);
    });
    await act(async () => {
      settle?.(respond(200, CLIENTS[4]));
    });
    await flush();

    expect(calls().length).toBe(before);
  });

  // F14 — one busy slot meant starting a second row's mutation blanked the
  // first row's busy state mid-flight; a set keeps both rows busy until each
  // one settles.
  it('keeps both rows busy while two mutations are in flight', async () => {
    const pending: Array<(value: Response) => void> = [];
    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      if ((init?.method ?? 'GET') === 'PUT') {
        return new Promise<Response>((resolve) => {
          pending.push(resolve);
        });
      }
      return Promise.resolve(route(path, init));
    });
    const dom = await mount();

    const rename = async (index: number) => {
      await click(rows(dom)[index]?.querySelector('.iconbtn'));
      await click(
        [...(rows(dom)[index]?.querySelectorAll('button') ?? [])].find(
          (button) => button.textContent === 'Rename',
        ),
      );
      await click(
        [...(rows(dom)[index]?.querySelectorAll('button') ?? [])].find(
          (button) => button.textContent === 'Save',
        ),
      );
    };
    await rename(0);
    await rename(4);

    const glyph = (index: number) =>
      rows(dom)[index]?.querySelector<HTMLButtonElement>('.iconbtn');
    expect(pending).toHaveLength(2);
    expect(glyph(0)?.disabled).toBe(true);
    expect(glyph(4)?.disabled).toBe(true);

    await act(async () => {
      pending[0]?.(respond(200, CLIENTS[0]));
    });
    await flush();
    expect(glyph(0)?.disabled).toBe(false);
    expect(glyph(4)?.disabled).toBe(true);

    await act(async () => {
      pending[1]?.(respond(200, CLIENTS[4]));
    });
    await flush();
    expect(glyph(4)?.disabled).toBe(false);
  });

  it('reports a failed re-read instead of silently keeping stale rows', async () => {
    const dom = await mount();
    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      if ((init?.method ?? 'GET') === 'PUT') {
        return Promise.resolve(route(path, init));
      }
      return Promise.reject(new TypeError('offline'));
    });
    await click(rows(dom)[4]?.querySelector('.iconbtn'));
    await click(
      [...(rows(dom)[4]?.querySelectorAll('button') ?? [])].find(
        (button) => button.textContent === 'Rename',
      ),
    );
    await click(
      [...dom.querySelectorAll('button')].find(
        (button) => button.textContent === 'Save',
      ),
    );
    expect(rows(dom).length).toBe(5);
    expect(dom.querySelector('.error-state-message')?.textContent).toContain(
      'did not reach the server',
    );
  });
});
