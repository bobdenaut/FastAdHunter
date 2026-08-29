// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setUnauthorizedHandler } from '../api/core';
import type { Client, PoliciesResponse, RuleTestResult } from '../api/types';
import type { Route } from '../router/routes';
import RuleTester from './rule-tester';
import { pushRecord, RING_LIMIT } from './rule-tester/session-ring';
import type { TestRecord } from './rule-tester/result-card';

/**
 * The capability under test is D5's condition: a name must reach the policy
 * that is actually in force for that client, and must never look like it
 * worked when what ran was the imperfect fallback. `test_rule` selects a
 * policy only on the address branch, so a name sent raw always answers
 * `default` — which is why one match substitutes the address, two block, and
 * none is marked partial.
 */

const ROUTE: Route = {
  path: '/rule-tester',
  title: 'Rule Tester',
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
    last_seen: '2026-08-27T10:00:00Z',
    queries_24h: 1,
    blocked_24h: 0,
    policy: 'default',
    ...overrides,
  };
}

const CLIENTS: Client[] = [
  client({
    ip: '192.168.10.50',
    name: 'tv',
    policy: 'kids',
    assignment_source: 'direct',
  }),
  client({ ip: '192.168.10.4', name: 'desktop' }),
  client({ ip: '192.168.20.11', policy: 'guest' }),
];

const POLICIES: PoliciesResponse = {
  timezone: 'UTC0',
  active_assignments: 1,
  items: [
    {
      id: 'kids',
      name: 'Kids',
      lists: null,
      blocking_mode: null,
      assignments: [
        { client: '192.168.10.50', days: 'mon-fri', start: '21:00', end: '07:00' },
      ],
    },
    {
      id: 'guest',
      name: 'Guest',
      lists: null,
      blocking_mode: null,
      assignments: [{ client: '192.168.20.0/24' }],
    },
  ],
};

const BLOCKED: RuleTestResult = {
  verdict: 'block',
  rule: '||vendor.net^$third-party',
  list: 'hagezi-pro',
  policy: 'kids',
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
let testResult: RuleTestResult = BLOCKED;

async function flush(): Promise<void> {
  await act(async () => {
    for (let turn = 0; turn < 8; turn += 1) await Promise.resolve();
  });
}

async function mount(): Promise<HTMLElement> {
  host = document.createElement('div');
  document.body.append(host);
  await act(async () => {
    render(<RuleTester route={ROUTE} />, host as HTMLElement);
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

function sent(index: number): Record<string, unknown> {
  return JSON.parse(
    (fetchMock.mock.calls[index]?.[1] as RequestInit).body as string,
  ) as Record<string, unknown>;
}

function byText(dom: HTMLElement, label: string): HTMLButtonElement {
  const found = [...dom.querySelectorAll('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (found === undefined) throw new Error(`no button: ${label}`);
  return found;
}

async function click(node: Element | null | undefined): Promise<void> {
  if (node === null || node === undefined) throw new Error('nothing to click');
  await act(async () => {
    (node as HTMLElement).click();
  });
  await flush();
}

async function fill(node: Element | null, value: string): Promise<void> {
  const field = node as HTMLInputElement;
  await act(async () => {
    field.value = value;
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function ask(
  dom: HTMLElement,
  domain: string,
  subject: string,
): Promise<void> {
  await fill(dom.querySelector('#tester-domain'), domain);
  await fill(
    dom.querySelector('input[aria-label="Client address or name"]'),
    subject,
  );
  await click(byText(dom, 'Test'));
}

function kv(dom: HTMLElement): Record<string, string> {
  const cells = [...(dom.querySelector('.kv')?.children ?? [])].map(
    (node) => node.textContent ?? '',
  );
  const out: Record<string, string> = {};
  for (let index = 0; index + 1 < cells.length; index += 2) {
    out[cells[index] ?? ''] = cells[index + 1] ?? '';
  }
  return out;
}

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  setUnauthorizedHandler(null);
  testResult = BLOCKED;
  fetchMock.mockImplementation((path: string, init?: RequestInit) => {
    if (path === '/api/v1/policies') return Promise.resolve(respond(200, POLICIES));
    if (path === '/api/v1/clients') {
      return Promise.resolve(respond(200, { items: CLIENTS }));
    }
    if (path === '/api/v1/rules/test' && init?.method === 'POST') {
      return Promise.resolve(respond(200, testResult));
    }
    throw new Error(`unexpected ${path}`);
  });
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
  it('reads two endpoints on mount and sends nothing until asked', async () => {
    await mount();
    expect(calls()).toEqual([
      ['GET', '/api/v1/policies'],
      ['GET', '/api/v1/clients'],
    ]);
  });
});

describe('client mode by address', () => {
  it('renders all four API fields plus the derived reason', async () => {
    const dom = await mount();
    await ask(dom, ' Metrics.Vendor.NET ', '192.168.10.50');
    // Trimmed and lowercased client-side, the way the handler does it, so the
    // echo on the card is the domain that was actually tested.
    expect(sent(2)).toEqual({
      domain: 'metrics.vendor.net',
      qtype: 'A',
      client: '192.168.10.50',
    });
    expect(dom.querySelector('.verdict-slab')?.textContent).toBe('block');
    expect(kv(dom)).toEqual({
      'matching rule': '||vendor.net^$third-party',
      'from list': 'hagezi-pro',
      'deciding policy': 'kids',
      'why that policy':
        'assignment on this address · mon–fri · 21:00 → 07:00 · in force now',
    });
  });

  it('renders a pass with both null fields as words, not as a failure', async () => {
    const dom = await mount();
    testResult = { verdict: 'pass', rule: null, list: null, policy: 'default' };
    await ask(dom, 'github.com', '192.168.10.4');
    expect(kv(dom)['matching rule']).toBe('no rule matched');
    expect(kv(dom)['from list']).toBe('—');
    expect(kv(dom)['why that policy']).toBe('no assignment covers this address');
  });

  it('names the user document the way the page that owns it does', async () => {
    const dom = await mount();
    testResult = {
      verdict: 'allow',
      rule: '@@||goodsite.example.com^',
      list: 'user-rules',
      policy: 'default',
    };
    await ask(dom, 'goodsite.example.com', '192.168.10.4');
    expect(kv(dom)['from list']).toBe('your custom rules');
  });

  // F13 — a blank client field is a default-context test, not a chosen policy:
  // the "why" must not borrow policy mode's sentence.
  it('says the default policy decides when the client field is blank', async () => {
    const dom = await mount();
    testResult = { verdict: 'pass', rule: null, list: null, policy: 'default' };
    await ask(dom, 'github.com', '');
    expect(sent(2)).toEqual({ domain: 'github.com', qtype: 'A' });
    expect(kv(dom)['why that policy']).toBe(
      'no client given — the default policy decides',
    );
    expect(dom.querySelector('.ring-row')?.textContent).toContain(
      'asthe default policy',
    );
  });
});

describe('D5 — a name resolved to an address', () => {
  it('substitutes the address visibly, folding ASCII case', async () => {
    const dom = await mount();
    await ask(dom, 'metrics.vendor.net', 'TV');
    // `TV` finds `tv` — the engine's own `eq_ignore_ascii_case`.
    expect(sent(2)['client']).toBe('192.168.10.50');
    expect(kv(dom)['tested as']).toBe('192.168.10.50 (tv)');
    expect(kv(dom)['deciding policy']).toBe('kids');
    expect(dom.querySelector('.banner')).toBeNull();
  });

  it('blocks on two matches and sends nothing until one is chosen', async () => {
    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      if (path === '/api/v1/policies') {
        return Promise.resolve(respond(200, POLICIES));
      }
      if (path === '/api/v1/clients') {
        return Promise.resolve(
          respond(200, {
            items: [
              ...CLIENTS,
              client({ ip: '192.168.10.51', name: 'TV', policy: 'guest' }),
            ],
          }),
        );
      }
      if (path === '/api/v1/rules/test' && init?.method === 'POST') {
        return Promise.resolve(respond(200, testResult));
      }
      throw new Error(`unexpected ${path}`);
    });
    const dom = await mount();
    await ask(dom, 'metrics.vendor.net', 'tv');

    // No request at all: there is no correct single answer to pick.
    expect(calls()).toHaveLength(2);
    expect(dom.querySelector('.tester-ambiguous')?.textContent).toContain(
      '2 observed clients answer to that name',
    );
    expect(dom.querySelector('.tester-result')).toBeNull();

    await click(dom.querySelectorAll('.ambiguous-choice')[1]);
    expect(sent(2)['client']).toBe('192.168.10.51');
    expect(dom.querySelector('.tester-ambiguous')).toBeNull();

    // F20 — the parenthetical names the client that was picked. Typing `tv`
    // and choosing the second match used to print `192.168.10.51 (tv)`, which
    // is the *other* client's name beside this one's address.
    expect(kv(dom)['tested as']).toBe('192.168.10.51 (TV)');
    // And the field carries the address the test actually used, so pressing
    // Test again asks the same question instead of re-opening the prompt.
    expect(
      (dom.querySelectorAll('input')[1] as HTMLInputElement).value,
    ).toBe('192.168.10.51');
  });

  // F20 — the previous answer used to stay on screen directly under the new
  // question, still answering the query before it.
  it('takes the previous result down while a choice is pending', async () => {
    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      if (path === '/api/v1/policies') {
        return Promise.resolve(respond(200, POLICIES));
      }
      if (path === '/api/v1/clients') {
        return Promise.resolve(
          respond(200, {
            items: [
              ...CLIENTS,
              client({ ip: '192.168.10.51', name: 'TV', policy: 'guest' }),
            ],
          }),
        );
      }
      if (path === '/api/v1/rules/test' && init?.method === 'POST') {
        return Promise.resolve(respond(200, testResult));
      }
      throw new Error(`unexpected ${path}`);
    });
    const dom = await mount();
    await ask(dom, 'metrics.vendor.net', '192.168.10.4');
    expect(dom.querySelector('.tester-result')).not.toBeNull();

    await ask(dom, 'metrics.vendor.net', 'tv');
    expect(dom.querySelector('.tester-ambiguous')).not.toBeNull();
    expect(dom.querySelector('.tester-result')).toBeNull();
  });

  it('marks an unobserved name partial and refuses to present `default` as an answer', async () => {
    const dom = await mount();
    testResult = { verdict: 'block', rule: '||ads^', list: 'oisd', policy: 'default' };
    await ask(dom, 'ads.example.com', 'kitchen-tablet');
    expect(sent(2)['client']).toBe('kitchen-tablet');
    const banner = dom.querySelector('.banner');
    expect(banner?.textContent).toContain('Partial answer');
    expect(banner?.textContent).toContain(
      'deciding policy reads default whatever is assigned',
    );
    expect(kv(dom)['why that policy']).toBe(
      'no address was given, so no assignment could apply',
    );
  });
});

describe('policy mode', () => {
  it('sends the policy, never a client, and says assignments are ignored', async () => {
    const dom = await mount();
    await click(byText(dom, 'a policy'));
    await fill(dom.querySelector('#tester-domain'), 'metrics.vendor.net');
    const select = dom.querySelector(
      'select[aria-label="Policy"]',
    ) as HTMLSelectElement;
    await act(async () => {
      select.value = 'guest';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await click(byText(dom, 'Test'));
    expect(sent(2)).toEqual({
      domain: 'metrics.vendor.net',
      qtype: 'A',
      policy: 'guest',
    });
    expect('client' in sent(2)).toBe(false);
    expect(kv(dom)['why that policy']).toBe(
      'you chose this policy — assignments are ignored',
    );
  });

  it('offers `default`, which the handler accepts explicitly', async () => {
    const dom = await mount();
    await click(byText(dom, 'a policy'));
    await fill(dom.querySelector('#tester-domain'), 'example.com');
    await click(byText(dom, 'Test'));
    expect(sent(2)['policy']).toBe('default');
  });
});

describe('the session ring', () => {
  it('is bounded at ten, newest first', () => {
    let ring: readonly TestRecord[] = [];
    for (let index = 0; index < 40; index += 1) {
      ring = pushRecord(ring, {
        domain: `d${String(index)}.example`,
        qtype: 'A',
        sentClient: '192.168.10.4',
        resolvedFrom: null,
        sentPolicy: null,
        result: BLOCKED,
        why: 'inherited',
        partial: false,
      });
    }
    expect(ring).toHaveLength(RING_LIMIT);
    expect(ring[0]?.domain).toBe('d39.example');
    expect(ring[RING_LIMIT - 1]?.domain).toBe('d30.example');
  });

  it('records each answered test', async () => {
    const dom = await mount();
    await ask(dom, 'a.example', '192.168.10.4');
    await ask(dom, 'b.example', '192.168.10.4');
    expect(dom.querySelectorAll('.ring-row')).toHaveLength(2);
    expect(dom.querySelector('.ring-row .mono')?.textContent).toBe('b.example');
  });
});
