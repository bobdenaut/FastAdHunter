// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  resetRestartBanner,
  restartArming,
} from '../../system/restart-banner';
import { InterceptionCard, entriesOf } from './interception-card';

/**
 * The Interception Document as a Settings card: one read on mount, one
 * whole-document write on save, errors placed from `details` rather than parsed
 * out of `message`, and no restart anywhere — because there is nothing to
 * restart.
 */

function respond(status: number, body: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: () => null },
    json: async () => body,
  } as unknown as Response;
}

interface Harness {
  requests: () => string[];
  bodies: () => unknown[];
}

let host: HTMLElement | null = null;
let harness: Harness | null = null;

function install(options: {
  document?: { clients: string[]; exclude_domains: string[] };
  put?: { status: number; body: unknown };
}): void {
  const requests: string[] = [];
  const bodies: unknown[] = [];
  const document = options.document ?? {
    clients: ['192.168.88.10'],
    exclude_domains: ['bank.ro'],
  };
  vi.stubGlobal(
    'fetch',
    vi.fn((url: string, init?: RequestInit) => {
      requests.push(`${init?.method ?? 'GET'} ${url}`);
      if (url !== '/api/v1/interception') {
        throw new Error(`unexpected ${url}`);
      }
      if (init?.method === 'PUT') {
        const sent = JSON.parse(String(init.body)) as unknown;
        bodies.push(sent);
        return Promise.resolve(
          options.put === undefined
            ? respond(200, sent)
            : respond(options.put.status, options.put.body),
        );
      }
      return Promise.resolve(respond(200, document));
    }),
  );
  harness = { requests: () => [...requests], bodies: () => [...bodies] };
}

async function flush(): Promise<void> {
  await act(async () => {
    for (let turn = 0; turn < 12; turn += 1) await Promise.resolve();
  });
}

async function mountCard(
  options: Parameters<typeof install>[0] & { mode?: string } = {},
): Promise<HTMLElement> {
  install(options);
  host = document.createElement('div');
  document.body.append(host);
  act(() => {
    render(
      <InterceptionCard mode={options.mode ?? 'dns+http+https'} />,
      host as HTMLElement,
    );
  });
  await flush();
  return host;
}

function editors(dom: HTMLElement): HTMLTextAreaElement[] {
  return [...dom.querySelectorAll('textarea')];
}

async function type(area: HTMLTextAreaElement, text: string): Promise<void> {
  await act(async () => {
    area.value = text;
    area.dispatchEvent(new Event('input', { bubbles: true }));
    await Promise.resolve();
  });
}

function button(dom: HTMLElement, label: string): HTMLButtonElement {
  const found = [...dom.querySelectorAll('button')].find(
    (node) => (node.textContent ?? '').trim() === label,
  );
  expect(found, label).toBeDefined();
  return found as HTMLButtonElement;
}

async function press(dom: HTMLElement, label: string): Promise<void> {
  await act(async () => {
    button(dom, label).dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await Promise.resolve();
  });
  await flush();
}

function callouts(dom: HTMLElement): string[] {
  return [...dom.querySelectorAll('.editor-callout')].map(
    (node) => node.textContent ?? '',
  );
}

afterEach(() => {
  if (host !== null) {
    act(() => {
      render(null, host as HTMLElement);
    });
    host.remove();
    host = null;
  }
  harness = null;
  vi.unstubAllGlobals();
});

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn());
});

describe('the entries a buffer sends', () => {
  it('drops blank lines and keeps the line each entry came from', () => {
    expect(entriesOf('a\n\n  \nb')).toEqual([
      { text: 'a', line: 1 },
      { text: 'b', line: 4 },
    ]);
    expect(entriesOf('')).toEqual([]);
  });
});

describe('the card', () => {
  it('reads once and shows both lists one entry per line', async () => {
    const dom = await mountCard({
      document: {
        clients: ['192.168.88.10', '192.168.88.0/24'],
        exclude_domains: ['bank.ro'],
      },
    });
    expect(harness?.requests()).toEqual(['GET /api/v1/interception']);
    const [clients, exclude] = editors(dom);
    expect(clients?.value).toBe('192.168.88.10\n192.168.88.0/24');
    expect(exclude?.value).toBe('bank.ro');
    expect(dom.textContent).toContain('2 / 256');
    expect(dom.textContent).toContain('1 / 512');
  });

  it('never names restart_required and never arms the banner', async () => {
    // The response carries no `restart_required` because there is nothing to
    // restart: a save applies on the next accepted connection. The card says
    // exactly that, and touches the banner in neither direction.
    resetRestartBanner();
    const dom = await mountCard({
      document: { clients: [], exclude_domains: [] },
    });
    await type(editors(dom)[0] as HTMLTextAreaElement, '10.0.0.1');
    await press(dom, 'Save document');

    expect(dom.textContent).not.toContain('restart_required');
    expect(dom.textContent).toContain('there is nothing to restart');
    expect(dom.textContent).toContain('Applied on the next connection.');
    expect(restartArming()).toBeNull();
  });

  it('sends the whole document as displayed, blank lines dropped', async () => {
    const dom = await mountCard({
      document: { clients: [], exclude_domains: [] },
    });
    const [clients, exclude] = editors(dom);
    await type(clients as HTMLTextAreaElement, '192.168.88.10\n\n10.0.0.0/8');
    await type(exclude as HTMLTextAreaElement, 'bank.ro\n');
    await press(dom, 'Save document');

    expect(harness?.bodies()).toEqual([
      {
        clients: ['192.168.88.10', '10.0.0.0/8'],
        exclude_domains: ['bank.ro'],
      },
    ]);
    // Rebased on the response, so the card is pristine again and says what the
    // save actually did.
    expect(dom.textContent).toContain('Applied on the next connection.');
    expect(button(dom, 'Save document').disabled).toBe(true);
    expect(editors(dom)[0]?.value).toBe('192.168.88.10\n10.0.0.0/8');
  });

  it('anchors an invalid entry on the line it was typed on', async () => {
    const dom = await mountCard({
      document: { clients: [], exclude_domains: [] },
      put: {
        status: 422,
        body: {
          error: {
            code: 'validation_failed',
            message: 'clients[1]: "10.0.0.300" is not an IP address or CIDR block',
            details: {
              reason: 'invalid_entry',
              list: 'clients',
              index: 1,
              entry: '10.0.0.300',
            },
          },
        },
      },
    });
    // A blank line above the offending entry: index 1 is line 4, and anchoring
    // on `index + 1` would put the band two lines high.
    await type(editors(dom)[0] as HTMLTextAreaElement, '10.0.0.1\n\n\n10.0.0.300');
    await press(dom, 'Save document');

    expect(callouts(dom)).toEqual(['line 4 — 10.0.0.300 is not an IP address or CIDR block']);
    expect(dom.textContent).toContain('your text is exactly as you left it');
    expect(button(dom, 'Save document').disabled).toBe(false);
  });

  it('anchors both lines of a duplicate', async () => {
    const dom = await mountCard({
      document: { clients: [], exclude_domains: [] },
      put: {
        status: 422,
        body: {
          error: {
            code: 'validation_failed',
            message: 'exclude_domains[2] duplicates entry 0',
            details: {
              reason: 'duplicate',
              list: 'exclude_domains',
              index: 2,
              entry: 'Bank.ro.',
              duplicate_of: 0,
            },
          },
        },
      },
    });
    await type(
      editors(dom)[1] as HTMLTextAreaElement,
      'bank.ro\nother.example\nBank.ro.',
    );
    await press(dom, 'Save document');

    expect(callouts(dom)).toEqual([
      'line 3 — Bank.ro. duplicates an earlier entry',
      'line 1 — the earlier entry it duplicates',
    ]);
  });

  it('states an over-cap rejection as one card line, not as an anchor', async () => {
    const dom = await mountCard({
      document: { clients: [], exclude_domains: [] },
      put: {
        status: 422,
        body: {
          error: {
            code: 'validation_failed',
            message: 'clients: 300 entries exceed the cap of 256 by 44',
            details: { reason: 'over_cap', list: 'clients', len: 300, cap: 256 },
          },
        },
      },
    });
    await type(editors(dom)[0] as HTMLTextAreaElement, '10.0.0.1');
    await press(dom, 'Save document');

    expect(dom.textContent).toContain('clients holds 300 entries and the cap is 256');
    expect(callouts(dom)).toEqual([]);
    expect(editors(dom)[0]?.value).toBe('10.0.0.1');
  });

  it('renders a 503 verbatim and a 500 with the contract’s own words', async () => {
    const dom = await mountCard({
      document: { clients: [], exclude_domains: [] },
      put: {
        status: 503,
        body: {
          error: {
            code: 'unavailable',
            message: 'the certificate store did not open',
          },
        },
      },
    });
    await type(editors(dom)[0] as HTMLTextAreaElement, '10.0.0.1');
    await press(dom, 'Save document');
    expect(dom.textContent).toContain('the certificate store did not open');

    await act(async () => {
      render(null, host as HTMLElement);
    });
    const other = await mountCard({
      document: { clients: [], exclude_domains: [] },
      put: {
        status: 500,
        body: {
          error: {
            code: 'internal',
            message: 'the document could not be written',
          },
        },
      },
    });
    await type(editors(other)[0] as HTMLTextAreaElement, '10.0.0.1');
    await press(other, 'Save document');
    expect(other.textContent).toContain('the document could not be written');
    expect(other.textContent).toContain('Nothing was applied.');
  });

  it('drops the rejection the moment a buffer is edited again', async () => {
    const dom = await mountCard({
      document: { clients: [], exclude_domains: [] },
      put: {
        status: 422,
        body: {
          error: {
            code: 'validation_failed',
            message: 'clients[0] is not an address',
            details: {
              reason: 'invalid_entry',
              list: 'clients',
              index: 0,
              entry: 'nope',
            },
          },
        },
      },
    });
    await type(editors(dom)[0] as HTMLTextAreaElement, 'nope');
    await press(dom, 'Save document');
    expect(callouts(dom)).toHaveLength(1);

    // Every line number in a rejection addresses the document as it was sent.
    await type(editors(dom)[0] as HTMLTextAreaElement, '10.0.0.1');
    expect(callouts(dom)).toEqual([]);
  });

  it('restores the last stored document on reset', async () => {
    const dom = await mountCard({
      document: { clients: ['10.0.0.1'], exclude_domains: [] },
    });
    await type(editors(dom)[0] as HTMLTextAreaElement, '10.0.0.1\n10.0.0.2');
    expect(button(dom, 'Reset').disabled).toBe(false);
    await press(dom, 'Reset');
    expect(editors(dom)[0]?.value).toBe('10.0.0.1');
    expect(harness?.bodies()).toEqual([]);
  });

  it('says the document is inert in a mode with no HTTPS listener', async () => {
    const dom = await mountCard({ mode: 'dns+http' });
    expect(dom.textContent).toContain('runs no HTTPS listener');
    expect(dom.textContent).toContain('dns+http');
  });

  it('says nothing of the sort once the mode has one', async () => {
    const dom = await mountCard({ mode: 'dns+http+https' });
    expect(dom.textContent).not.toContain('runs no HTTPS listener');
  });
});
