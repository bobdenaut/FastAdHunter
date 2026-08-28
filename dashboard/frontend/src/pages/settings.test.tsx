// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Route } from '../router/routes';
import { refresh, socket } from '../services';
import { LOGIN_PATH } from '../session/session';
import {
  armRestartBanner,
  resetRestartBanner,
  restartArming,
} from '../system/restart-banner';
import Settings from './settings';

/**
 * The acceptance criteria this page owns, read off the DOM and off the `fetch`
 * body: only the changed keys travel, the raw panel prints no auth material,
 * and `[api]` cannot be saved without the consequence being named first.
 */

const ROUTE: Route = {
  path: '/settings',
  title: 'Settings',
  section: 'system',
  events: ['config_changed'],
  endpoints: [],
  built: true,
  ownsHeader: true,
  load: null,
};

function config(over: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    engine: { mode: 'dns' },
    dns: {
      listen: { address: '::', port: 53 },
      blocking: { mode: 'null_ip', ttl_seconds: 10 },
      cache: {
        max_entries: 10_000,
        max_bytes: 67_108_864,
        min_ttl_seconds: 0,
        max_ttl_seconds: 86_400,
        negative_ttl_max_seconds: 60,
        serve_stale: true,
        swr_workers: 3,
        cleanup_interval_seconds: 360,
      },
      upstreams: {
        strategy: 'fallback',
        timeout_ms: 800,
        penalty_failures: 2,
        servers: [{ address: '1.1.1.1', protocol: 'udp', hostname: null }],
      },
    },
    http: {
      listen: { address: '::', port: 8080 },
      max_connections: 1024,
      idle_timeout_ms: 60_000,
      header_timeout_ms: 10_000,
    },
    egress: { allow_destinations: [], allow_ip_literal_hosts: false },
    rules: {
      refresh_hours_default: 24,
      lists: [
        { id: 'oisd-basic', url: 'https://small.oisd.nl', enabled: true, refresh_hours: null },
      ],
    },
    schedule: { timezone: 'UTC' },
    stats: { snapshot_interval_seconds: 300 },
    history: { enabled: true, sample_interval_seconds: 60, retention_days: 30 },
    api: { address: '0.0.0.0', port: 8443, tls: true },
    log: { level: 'info', format: 'text' },
    ...over,
  };
}

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

interface Harness {
  postBodies: () => unknown[];
  configReads: () => number;
  healthReads: () => number;
  /** Every request the page made, in order — what "and nothing else happened"
   *  is read off. */
  requests: () => string[];
}

let host: HTMLElement | null = null;
let harness: Harness | null = null;

function install(options: {
  document?: Record<string, unknown>;
  postStatus?: number;
  postBody?: unknown;
}): Harness {
  const posts: unknown[] = [];
  const requests: string[] = [];
  let configReads = 0;
  let healthReads = 0;
  const document = options.document ?? config();
  const fetchMock = vi.fn((url: string, init?: RequestInit) => {
    requests.push(`${init?.method ?? 'GET'} ${url}`);
    if (url === '/api/v1/config' && init?.method === 'POST') {
      posts.push(JSON.parse(String(init.body)));
      return Promise.resolve(
        respond(
          options.postStatus ?? 200,
          options.postBody ?? { applied: false, restart_required: true },
        ),
      );
    }
    if (url === '/api/v1/config') {
      configReads += 1;
      return Promise.resolve(respond(200, document));
    }
    if (url === '/health') {
      healthReads += 1;
      return Promise.resolve(
        respond(200, { status: 'ok', version: '0.2.20', uptime_seconds: 5 }),
      );
    }
    if (url === '/api/v1/auth/logout-all') return Promise.resolve(respond(204));
    if (url === '/api/v1/config/apikey/rotate') {
      return Promise.resolve(respond(200, { api_key: 'fah_rotated_secret' }));
    }
    if (url === '/api/v1/auth/password') return Promise.resolve(respond(204));
    throw new Error(`unexpected ${url}`);
  });
  vi.stubGlobal('fetch', fetchMock);
  const store = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => store.set(key, value),
  });
  harness = {
    postBodies: () => posts,
    configReads: () => configReads,
    healthReads: () => healthReads,
    requests: () => [...requests],
  };
  return harness;
}

async function flush(): Promise<void> {
  await act(async () => {
    for (let turn = 0; turn < 12; turn += 1) await Promise.resolve();
  });
}

async function mountPage(options: Parameters<typeof install>[0] = {}): Promise<HTMLElement> {
  install(options);
  host = document.createElement('div');
  document.body.append(host);
  act(() => {
    render(<Settings route={ROUTE} />, host as HTMLElement);
  });
  await flush();
  return host;
}

function field(dom: HTMLElement, key: string): HTMLElement {
  const node = dom.querySelector(`#set-${key.replace(/\./g, '-')}`);
  expect(node, key).not.toBeNull();
  return node as HTMLElement;
}

async function type(dom: HTMLElement, key: string, value: string): Promise<void> {
  const input = field(dom, key) as HTMLInputElement;
  await act(async () => {
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
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

/** The dotted key of every row in the All-settings panel. Reading the keys
 *  rather than the panel's whole text keeps the prose out of the assertion —
 *  the footnote naming `auth` is what it says about auth, not a row. */
function rawKeys(dom: HTMLElement): string[] {
  return [...dom.querySelectorAll('.raw-row')].map(
    (row) => row.firstElementChild?.textContent ?? '',
  );
}

async function click(node: HTMLElement): Promise<void> {
  await act(async () => {
    node.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await Promise.resolve();
  });
  await flush();
}

beforeEach(() => {
  resetRestartBanner();
});

afterEach(() => {
  if (host !== null) {
    act(() => {
      render(null, host as HTMLElement);
    });
    host.remove();
    host = null;
  }
  harness = null;
  resetRestartBanner();
  vi.unstubAllGlobals();
});

describe('the write body', () => {
  it('carries only the changed key, nested — the acceptance criterion', async () => {
    const dom = await mountPage();
    await type(dom, 'dns.cache.max_entries', '20000');
    await click(button(dom, 'Save changes'));
    expect(harness?.postBodies()).toEqual([
      { dns: { cache: { max_entries: 20_000 } } },
    ]);
  });

  it('never sends the document it read back', async () => {
    const dom = await mountPage();
    await type(dom, 'history.retention_days', '60');
    await click(button(dom, 'Save changes'));
    const [body] = harness?.postBodies() ?? [];
    expect(Object.keys(body as object)).toEqual(['history']);
    expect(body).toEqual({ history: { retention_days: 60 } });
  });

  it('sends nothing at all when a value is typed back to its own', async () => {
    const dom = await mountPage();
    await type(dom, 'dns.cache.max_entries', '20000');
    await type(dom, 'dns.cache.max_entries', '10000');
    expect(button(dom, 'Save changes').disabled).toBe(true);
    expect(harness?.postBodies()).toEqual([]);
  });

  it('refuses an out-of-range value without a request', async () => {
    const dom = await mountPage();
    await type(dom, 'dns.upstreams.timeout_ms', '20000');
    await click(button(dom, 'Save changes'));
    expect(harness?.postBodies()).toEqual([]);
    expect(dom.querySelector('.set-field-error')?.textContent).toContain(
      'between 1 and 10,000',
    );
  });

  it('anchors a 422 under the field the server named', async () => {
    const dom = await mountPage({
      postStatus: 422,
      postBody: {
        error: {
          code: 'validation_failed',
          message: 'invalid value for `dns.cache.max_bytes`: must be at least 1048576 (got 4)',
        },
      },
    });
    await type(dom, 'dns.cache.max_bytes', '2097152');
    await click(button(dom, 'Save changes'));
    const anchored = field(dom, 'dns.cache.max_bytes').parentElement;
    expect(anchored?.querySelector('.set-field-error')?.textContent).toContain(
      'must be at least 1048576',
    );
  });
});

describe('the `[api]` gate', () => {
  it('sends nothing until the consequence is confirmed', async () => {
    const dom = await mountPage();
    const toggle = field(dom, 'api.tls') as HTMLInputElement;
    await act(async () => {
      toggle.checked = false;
      toggle.dispatchEvent(new Event('change', { bubbles: true }));
      await Promise.resolve();
    });
    await click(button(dom, 'Save changes'));
    expect(harness?.postBodies()).toEqual([]);
    expect(dom.querySelector('[role="dialog"]')?.textContent).toContain(
      'signing in stops working',
    );
  });

  it('sends nothing when the confirmation is cancelled', async () => {
    const dom = await mountPage();
    await type(dom, 'api.port', '9443');
    await click(button(dom, 'Save changes'));
    await click(button(dom, 'Cancel'));
    expect(harness?.postBodies()).toEqual([]);
    expect(dom.querySelector('[role="dialog"]')).toBeNull();
  });

  it('names the lock-out and then sends exactly the gated key', async () => {
    const dom = await mountPage();
    await type(dom, 'api.port', '9443');
    await click(button(dom, 'Save changes'));
    expect(dom.querySelector('[role="dialog"]')?.textContent).toContain(
      'stops answering',
    );
    await click(button(dom, 'Save anyway'));
    expect(harness?.postBodies()).toEqual([{ api: { port: 9443 } }]);
  });

  it('does not ask on an ordinary field', async () => {
    const dom = await mountPage();
    await type(dom, 'log.level', 'debug');
    await click(button(dom, 'Save changes'));
    expect(dom.querySelector('[role="dialog"]')).toBeNull();
    expect(harness?.postBodies()).toHaveLength(1);
  });
});

describe('the restart banner', () => {
  it('arms with the boot keys this browser submitted', async () => {
    const dom = await mountPage();
    await type(dom, 'dns.cache.max_entries', '20000');
    await click(button(dom, 'Save changes'));
    expect(restartArming()?.keys).toEqual(['dns.cache.max_entries']);
  });

  it('stays clear when the response applied the change live', async () => {
    const dom = await mountPage({
      postBody: { applied: true, restart_required: false },
    });
    await type(dom, 'history.retention_days', '60');
    await click(button(dom, 'Save changes'));
    expect(restartArming()).toBeNull();
    expect(dom.textContent).toContain('Applied live');
  });

  it('reads `/health` on entry only while something is pending', async () => {
    await mountPage();
    expect(harness?.healthReads()).toBe(0);
  });

  it('revalidates on entry when the banner is armed, and clears on a restart', async () => {
    armRestartBanner(['dns.cache.max_entries'], Date.now() - 3_600_000);
    await mountPage();
    // The stub answers `uptime_seconds: 5`, so this process booted long after
    // the arming.
    expect(harness?.healthReads()).toBe(1);
    expect(restartArming()).toBeNull();
  });
});

describe('the All settings panel', () => {
  it('shows keys the curated form does not model', async () => {
    // `rules.lists` has no field above — the endpoint 422s it — and an array is
    // one leaf, because the merge replaces arrays wholesale and a per-element
    // key would name something no patch can address.
    const dom = await mountPage();
    const raw = dom.querySelector('.raw-panel')?.textContent ?? '';
    expect(rawKeys(dom)).toContain('rules.lists');
    expect(raw).toContain('oisd-basic');
  });

  it('drops an `auth` key even though the endpoint omits it', async () => {
    const dom = await mountPage({
      document: config({ auth: { hash: '$argon2id$v=19$m=19456' } }),
    });
    expect(rawKeys(dom).filter((key) => key.startsWith('auth'))).toEqual([]);
    expect(dom.textContent).not.toContain('argon2id');
  });

  it('renders an absent `policies` key as "none configured"', async () => {
    const dom = await mountPage();
    const raw = dom.querySelector('.raw-panel')?.textContent ?? '';
    expect(rawKeys(dom)).toContain('policies');
    expect(raw).toContain('none configured');
  });

  it('renders a configured policy set rather than that wording', async () => {
    const dom = await mountPage({
      document: config({ policies: [{ id: 'kids', assignments: [] }] }),
    });
    const raw = dom.querySelector('.raw-panel')?.textContent ?? '';
    expect(raw).toContain('kids');
    expect(raw).not.toContain('none configured');
  });
});

describe('the Access panel', () => {
  it('warns before the rotation is confirmed, and shows the key once', async () => {
    const dom = await mountPage();
    await click(button(dom, 'Rotate'));
    const dialog = dom.querySelector('[role="dialog"]');
    expect(dialog?.textContent).toContain('shown once');
    expect(dialog?.textContent).toContain('breaks until it is updated');
    // Nothing has been rotated yet — the warning precedes the request.
    expect(dom.querySelector('[data-testid="rotated-key"]')).toBeNull();

    await click(button(dom, 'Rotate the key'));
    expect(
      dom.querySelector('[data-testid="rotated-key"]')?.textContent,
    ).toBe('fah_rotated_secret');
  });

  it('draws no masked key value anywhere', async () => {
    // No endpoint returns the current key; a mask would imply this browser
    // holds one.
    const dom = await mountPage();
    expect(dom.textContent).not.toContain('•');
    expect(dom.textContent).not.toContain('fah_••');
  });

  it('says every session dies before the password is changed', async () => {
    const dom = await mountPage();
    await click(button(dom, 'Change password'));
    expect(dom.querySelector('[role="dialog"]')?.textContent).toContain(
      'every signed-in browser is signed out',
    );
  });

  it('paints the signed-out state instead of navigating over it', async () => {
    // `onSignedOut()` fired in the same tick as `setDone(true)`, so the panel
    // explaining that the secret was rotated and no cookie was issued could
    // never appear. The operator dismisses it, which is also the only
    // acknowledgement the rotation gets.
    const dom = await mountPage();
    await click(button(dom, 'Change password'));
    const dialog = dom.querySelector('[role="dialog"]') as HTMLElement;
    const inputs = [...dialog.querySelectorAll('input')];
    for (const input of inputs) {
      await act(async () => {
        input.value = 'a-long-enough-password';
        input.dispatchEvent(new Event('input', { bubbles: true }));
        await Promise.resolve();
      });
    }
    await click(button(dialog, 'Change password'));

    const after = dom.querySelector('[role="dialog"]') as HTMLElement;
    expect(after.textContent).toContain('Every session was signed out');
    expect(button(after, 'Sign in again')).toBeDefined();
  });

  /**
   * A successful rotation, up to the done panel. The router scrolls on a
   * navigation and jsdom does not implement that, so it is stubbed here rather
   * than letting a warning stand in for the assertion.
   */
  async function rotate(dom: HTMLElement): Promise<HTMLElement> {
    vi.stubGlobal('scrollTo', () => undefined);
    window.history.replaceState(null, '', '/settings');
    await click(button(dom, 'Change password'));
    const dialog = dom.querySelector('[role="dialog"]') as HTMLElement;
    for (const input of [...dialog.querySelectorAll('input')]) {
      await act(async () => {
        input.value = 'a-long-enough-password';
        input.dispatchEvent(new Event('input', { bubbles: true }));
        await Promise.resolve();
      });
    }
    await click(button(dialog, 'Change password'));
    return dom.querySelector('[role="dialog"]') as HTMLElement;
  }

  it('does not let Escape dismiss the done panel back to the page', async () => {
    // The request that succeeded revoked this browser's cookie. Escaping the
    // panel to the form behind it left the operator reading Settings under a
    // session that is already gone, which is the state the panel exists to
    // announce — so once it is up, leaving means signing out.
    const dom = await mountPage();
    await rotate(dom);
    await act(async () => {
      document.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }),
      );
      await Promise.resolve();
    });
    expect(dom.querySelector('[role="dialog"]')?.textContent).toContain(
      'Every session was signed out',
    );
    expect(window.location.pathname).toBe(LOGIN_PATH);
  });

  it('signs out when the done panel’s own action is pressed', async () => {
    const dom = await mountPage();
    const done = await rotate(dom);
    await click(button(done, 'Sign in again'));
    expect(window.location.pathname).toBe(LOGIN_PATH);
  });

  it('issues nothing between the rotation and the navigation', async () => {
    // The rotation is the last request this browser can make: its cookie is
    // gone. A re-read fired while the panel is up would 401 and land the
    // operator on the login screen through the failure path instead of the
    // sentence explaining what happened.
    const dom = await mountPage();
    const done = await rotate(dom);
    const settled = harness?.requests() ?? [];
    expect(settled[settled.length - 1]).toBe('POST /api/v1/auth/password');
    await click(button(done, 'Sign in again'));
    expect(harness?.requests()).toEqual(settled);
  });
});

describe('the page’s own lifecycle', () => {
  it('holds no timer while it is mounted', async () => {
    await mountPage();
    expect(refresh.activeTimers()).toBe(0);
  });

  it('reads the configuration once on entry', async () => {
    await mountPage();
    expect(harness?.configReads()).toBe(1);
  });

  it('re-reads it once after a save, and adopts the result as the baseline', async () => {
    // The server publishes `config_changed` for this write too. The
    // single-flight reader is what keeps the pair to one request; the rebase
    // that keeps an unsaved edit across the new document is asserted in
    // `settings/patch.test.ts`, where every branch is reachable.
    const dom = await mountPage();
    await type(dom, 'log.level', 'debug');
    await click(button(dom, 'Save changes'));
    expect(harness?.configReads()).toBe(2);
    expect(button(dom, 'Save changes').disabled).toBe(true);
  });

  it('states that the tags are carried rather than served', async () => {
    const dom = await mountPage();
    expect(dom.querySelector('.sub')?.textContent).toContain(
      'never from the response',
    );
  });
});

/**
 * The two controls a green suite had never touched: the upstream row editor and
 * the destination line editor. Both were unusable — one lost the caret on every
 * keystroke, the other undid a newline before the next character arrived — and
 * neither failure is visible to a validation test, which is all `patch.test.ts`
 * could offer. These drive the DOM.
 */
describe('the editors that hold a list', () => {
  /** Types into an arbitrary field, by `aria-label` rather than by id: the row
   *  editor's cells are not `FieldMeta` and carry no `#set-…` id. */
  async function typeInto(node: HTMLElement, value: string): Promise<void> {
    await act(async () => {
      (node as HTMLInputElement | HTMLTextAreaElement).value = value;
      node.dispatchEvent(new Event('input', { bubbles: true }));
      await Promise.resolve();
    });
  }

  function labelled(dom: HTMLElement, label: string): HTMLElement {
    const node = dom.querySelector(`[aria-label="${label}"]`);
    expect(node, label).not.toBeNull();
    return node as HTMLElement;
  }

  it('keeps the upstream address input across a keystroke, caret and all', async () => {
    // The row key used to carry `row.address`, so the field's own input
    // changed the key, Preact remounted the row, and the replacement input was
    // not the one being typed in. The list is positional — the index *is* the
    // answering endpoint — so position is the identity.
    const dom = await mountPage();
    const before = labelled(dom, 'Upstream 1 address');
    before.focus();
    expect(document.activeElement).toBe(before);

    await typeInto(before, '9.9.9.9');

    expect(labelled(dom, 'Upstream 1 address')).toBe(before);
    expect(document.activeElement).toBe(before);
    expect((before as HTMLInputElement).value).toBe('9.9.9.9');
  });

  it('sends the whole upstream array when one cell changes', async () => {
    const dom = await mountPage();
    await typeInto(labelled(dom, 'Upstream 1 address'), '9.9.9.9');
    await click(button(dom, 'Save changes'));
    expect(harness?.postBodies()).toEqual([
      {
        dns: {
          upstreams: {
            servers: [{ address: '9.9.9.9', protocol: 'udp', hostname: null }],
          },
        },
      },
    ]);
  });

  it('keeps a newline the operator types in the destination editor', async () => {
    // The editor is a fully controlled textarea. Parsing on every input and
    // re-rendering from the parsed array stripped the trailing empty line a
    // new entry starts on, so Enter was undone before the next character
    // arrived and a second destination could not be typed at all.
    const dom = await mountPage();
    const area = labelled(dom, 'egress.allow_destinations') as HTMLTextAreaElement;

    await typeInto(area, '10.0.0.0/8');
    expect(area.value).toBe('10.0.0.0/8');

    await typeInto(area, '10.0.0.0/8\n');
    expect(area.value).toBe('10.0.0.0/8\n');

    await typeInto(area, '10.0.0.0/8\n192.168.0.0/16');
    expect(area.value).toBe('10.0.0.0/8\n192.168.0.0/16');
  });

  it('commits the destination editor as trimmed, non-empty entries', async () => {
    // The raw buffer is what is edited; the parse happens on the way out, so a
    // blank line and a stray space never reach the patch.
    const dom = await mountPage();
    const area = labelled(dom, 'egress.allow_destinations');
    await typeInto(area, '10.0.0.0/8\n\n  192.168.0.0/16  \n');
    await click(button(dom, 'Save changes'));
    expect(harness?.postBodies()).toEqual([
      { egress: { allow_destinations: ['10.0.0.0/8', '192.168.0.0/16'] } },
    ]);
  });

  it('drops the draft when the baseline replaces it', async () => {
    // A Discard puts the server's value back under the editor; the draft is
    // only kept while it still parses to what arrived from above.
    const dom = await mountPage();
    const area = labelled(dom, 'egress.allow_destinations') as HTMLTextAreaElement;
    await typeInto(area, '10.0.0.0/8\n');
    await click(button(dom, 'Discard'));
    expect(
      (labelled(dom, 'egress.allow_destinations') as HTMLTextAreaElement).value,
    ).toBe('');
    expect(harness?.postBodies()).toEqual([]);
  });
});

describe('two reads in flight at once', () => {
  /**
   * The race the conditional join opened: a read provoked by a change may not
   * adopt one that left before it, so two `GET /config` can now overlap — and
   * two requests settle in whatever order the network gives them.
   */
  function withTtl(ttl: number): Record<string, unknown> {
    const document = config();
    const dns = document['dns'] as { blocking: { ttl_seconds: number } };
    dns.blocking.ttl_seconds = ttl;
    return document;
  }

  it('keeps the newer document when the older read settles last', async () => {
    const settle: Array<(body: unknown) => void> = [];
    vi.stubGlobal(
      'fetch',
      vi.fn((url: string, init?: RequestInit) => {
        if (url === '/api/v1/config' && init?.method !== 'POST') {
          return new Promise<Response>((resolve) => {
            settle.push((body) => resolve(respond(200, body)));
          });
        }
        throw new Error(`unexpected ${url}`);
      }),
    );
    const store = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => store.set(key, value),
    });
    // `startedAt` is `Date.now()`. Held still and stepped by hand, so the two
    // reads carry distinct instants rather than whatever the clock happens to
    // do inside one millisecond.
    let clock = 1_000;
    const now = vi.spyOn(Date, 'now').mockImplementation(() => clock);
    let announce: ((data: Record<string, unknown>) => void) | null = null;
    const on = vi.spyOn(socket, 'on').mockImplementation((type, listener) => {
      if (type === 'config_changed') {
        announce = listener as (data: Record<string, unknown>) => void;
      }
      return () => undefined;
    });

    host = document.createElement('div');
    document.body.append(host);
    act(() => {
      render(<Settings route={ROUTE} />, host as HTMLElement);
    });
    await flush();
    // A is away and unanswered.
    expect(settle).toHaveLength(1);

    clock = 2_000;
    await act(async () => {
      announce?.({ restart_required: false });
      await Promise.resolve();
    });
    // B did not join A: A left before the change, so it is not an answer to it.
    expect(settle).toHaveLength(2);

    // B answers first, A second — the order the guard exists for.
    await act(async () => {
      settle[1]?.(withTtl(42));
      await Promise.resolve();
    });
    await flush();
    await act(async () => {
      settle[0]?.(withTtl(10));
      await Promise.resolve();
    });
    await flush();

    const input = field(host, 'dns.blocking.ttl_seconds') as HTMLInputElement;
    expect(input.value).toBe('42');

    on.mockRestore();
    now.mockRestore();
  });
});
