// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Telemetry, Upstream } from '../api/types';
import type { Route } from '../router/routes';
import Upstreams from './upstreams';
import { DegradedBanner } from '../components/degraded-banner';
import { EndpointRow } from './upstreams/endpoint-row';
import { endpointOrder } from './upstreams/rtt-chart';
import { StatesCard } from './upstreams/states-card';

/**
 * The one thing this page must never do is present `fallback`'s zeros as
 * health. API.md is explicit that under that strategy every row publishes
 * `state: healthy`, `penalty_round: 0` and zeros for penalties, penalized
 * seconds, probes and probe successes because **no health state exists to
 * report** — which is not the same as "everything is fine".
 *
 * The cards are mounted directly; the page itself is two `useRefresh` calls,
 * one `/config` one-shot and a layout, and its route declaration is pinned in
 * `routes.test.ts`.
 */

function endpoint(over: Partial<Upstream> = {}): Upstream {
  return {
    address: '1.1.1.1:853',
    protocol: 'dot',
    attempts: 201_883,
    failures: 12,
    consecutive_failures: 0,
    tls_handshakes: 41,
    failure_runs: [2, 1, 0, 0],
    state: 'healthy',
    penalty_round: 0,
    penalties: 0,
    penalized_seconds_total: 0,
    probes: 0,
    probe_successes: 0,
    family: 'v4',
    ...over,
  };
}

let host: HTMLElement | null = null;

function mount(node: preact.ComponentChild): HTMLElement {
  host = document.createElement('div');
  document.body.append(host);
  act(() => {
    render(node, host as HTMLElement);
  });
  return host;
}

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn());
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

describe('an adaptive endpoint row', () => {
  it('carries the state pill and all eight counters', () => {
    const dom = mount(
      <EndpointRow index={0} upstream={endpoint()} mode="adaptive" />,
    );
    expect(dom.querySelector('.pill')?.textContent).toBe('healthy');
    expect(dom.querySelectorAll('.ep-counters > div')).toHaveLength(8);
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('201,883');
    expect(text).toContain('probe successes');
    // E21 — the unit travels with the figure; no duration arithmetic.
    expect(text).toContain('0 s');
  });

  it('names its index, which is the answering-endpoint identity', () => {
    const dom = mount(
      <EndpointRow index={2} upstream={endpoint()} mode="adaptive" />,
    );
    expect(dom.querySelector('.ep-index')?.textContent).toBe('2');
  });

  it('shows the penalty round only while the endpoint is penalized', () => {
    const healthy = mount(
      <EndpointRow
        index={0}
        upstream={endpoint({ penalty_round: 3 })}
        mode="adaptive"
      />,
    );
    // `penalty_round` is never cleared by recovery, so a healthy endpoint still
    // publishes the round it reached. Printing it would imply an active state.
    expect(healthy.textContent).not.toContain('round 3');
  });

  it('shows it on a penalized one, where it is the artboard’s own place', () => {
    const dom = mount(
      <EndpointRow
        index={1}
        upstream={endpoint({ state: 'penalized', penalty_round: 3 })}
        mode="adaptive"
      />,
    );
    expect(dom.textContent).toContain('round 3');
    expect(dom.querySelector('.ep.is-penalized')).not.toBeNull();
  });

  it('marks the live figure and leaves the cumulative totals neutral', () => {
    const dom = mount(
      <EndpointRow
        index={0}
        upstream={endpoint({ consecutive_failures: 7 })}
        mode="adaptive"
      />,
    );
    expect(dom.querySelector('.ep-counters .bad')?.textContent).toBe('7');
  });
});

describe('a fallback endpoint row', () => {
  it('omits the state pill and the four health cells', () => {
    const dom = mount(
      <EndpointRow index={0} upstream={endpoint()} mode="fallback" />,
    );
    expect(dom.querySelector('.pill')).toBeNull();
    expect(dom.querySelectorAll('.ep-counters > div')).toHaveLength(4);
    const text = dom.textContent ?? '';
    for (const absent of [
      'penalties',
      'penalized for',
      'probes',
      'probe successes',
    ]) {
      expect(text).not.toContain(absent);
    }
  });

  it('keeps the counters that mean the same thing under either strategy', () => {
    const dom = mount(
      <EndpointRow index={0} upstream={endpoint()} mode="fallback" />,
    );
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    for (const present of [
      'attempts',
      'failures',
      'consecutive',
      'TLS handshakes',
      'failure-run histogram',
    ]) {
      expect(text).toContain(present);
    }
  });
});

describe('the address family', () => {
  it('renders the unknown case as a word, never as `null`', () => {
    const dom = mount(
      <EndpointRow
        index={0}
        upstream={endpoint({
          address: 'https://dns.example.net/dns-query',
          protocol: 'doh',
          family: null,
        })}
        mode="adaptive"
      />,
    );
    const text = dom.textContent ?? '';
    expect(text).toContain('family unknown');
    expect(text).not.toContain('null');
  });

  it('prints the family verbatim when the config gave one', () => {
    const dom = mount(
      <EndpointRow index={0} upstream={endpoint({ family: 'v6' })} mode="adaptive" />,
    );
    expect(dom.querySelector('.ep-state .note')?.textContent).toContain(
      'dot · v6',
    );
  });
});

describe('the failure-run histogram', () => {
  it('normalises to the row’s own largest bucket', () => {
    const dom = mount(
      <EndpointRow
        index={1}
        upstream={endpoint({ failure_runs: [12, 21, 30, 39] })}
        mode="adaptive"
      />,
    );
    const heights = [...dom.querySelectorAll<HTMLElement>('.run-bar')].map(
      (node) => node.style.height,
    );
    expect(heights[3]).toBe('100%');
    expect(heights[0]).not.toBe('0%');
    expect((dom.textContent ?? '').replace(/\s+/g, ' ')).toContain(
      '12 · 21 · 30 · 39 runs of length 1,2,3,4+',
    );
  });

  it('draws four empty tracks when nothing has closed a run', () => {
    const dom = mount(
      <EndpointRow
        index={0}
        upstream={endpoint({ failure_runs: [0, 0, 0, 0] })}
        mode="adaptive"
      />,
    );
    expect(dom.querySelectorAll('.run-track')).toHaveLength(4);
    for (const bar of dom.querySelectorAll<HTMLElement>('.run-bar')) {
      expect(bar.style.height).toBe('0%');
    }
  });

  it('keeps every bar the same neutral colour at every state', () => {
    // The artboard tints the penalized row's bars amber and red and annotates
    // them "long runs dominate" — a severity the histogram alone cannot
    // support. The printed counts carry the figures instead.
    const dom = mount(
      <EndpointRow
        index={1}
        upstream={endpoint({ state: 'penalized', failure_runs: [12, 21, 30, 39] })}
        mode="adaptive"
      />,
    );
    for (const bar of dom.querySelectorAll<HTMLElement>('.run-bar')) {
      expect(bar.style.background).toBe('');
    }
    expect(dom.textContent).not.toContain('long runs dominate');
  });
});

describe('the degraded banner', () => {
  it('gives the adaptive reading under adaptive', () => {
    const dom = mount(<DegradedBanner mode="adaptive" />);
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('this is not an outage');
    expect(text).toContain('no endpoint is currently healthy');
    expect(text).toContain('Clients are being served.');
  });

  it('gives the fallback reading under fallback', () => {
    const dom = mount(<DegradedBanner mode="fallback" />);
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('non-zero consecutive-failure count');
    expect(text).not.toContain('no endpoint is currently healthy');
  });

  it('gives both readings, attributed, when the strategy is unreadable', () => {
    const dom = mount(<DegradedBanner mode="unknown" />);
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('The strategy could not be read');
    expect(text).toContain('no endpoint is currently healthy');
    expect(text).toContain('non-zero consecutive-failure count');
  });

  it('carries no refresh controls of its own', () => {
    // One cluster per polled endpoint: `/health` already has one in the page
    // header, and a second would show two ages for one reading.
    const dom = mount(<DegradedBanner mode="adaptive" />);
    expect(dom.querySelector('.ctl')).toBeNull();
    expect(dom.querySelector('button')).toBeNull();
  });
});

describe('the states legend', () => {
  it('explains the three pills under adaptive', () => {
    const dom = mount(<StatesCard mode="adaptive" />);
    expect(
      [...dom.querySelectorAll('.pill')].map((node) => node.textContent),
    ).toEqual(['healthy', 'penalized', 'probing']);
  });

  it('explains that there are no states under fallback', () => {
    const dom = mount(<StatesCard mode="fallback" />);
    expect(dom.querySelectorAll('.pill')).toHaveLength(0);
    expect(dom.textContent).toContain('no health state exists to report');
  });
});

/* ------------------------------------------------------- the page as a whole */

const ROUTE: Route = {
  path: '/upstreams',
  title: 'Upstreams',
  section: 'runtime',
  events: [],
  endpoints: ['telemetry', 'health'],
  built: true,
  ownsHeader: true,
  load: null,
};

const TELEMETRY = {
  upstreams: [endpoint(), endpoint({ address: '9.9.9.9:853', family: null })],
} as unknown as Telemetry;

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

async function flushPage(): Promise<void> {
  await act(async () => {
    for (let turn = 0; turn < 12; turn += 1) await Promise.resolve();
  });
}

/** `config` of `null` makes `GET /config` fail, which is C14's case. */
async function mountPage(
  config: unknown,
  status: 'ok' | 'degraded' = 'ok',
): Promise<HTMLElement> {
  const fetchMock = vi.fn((url: string) => {
    if (url === '/api/v1/config') {
      return Promise.resolve(
        config === null
          ? respond(500, { error: { code: 'internal', message: 'no' } })
          : respond(200, config),
      );
    }
    if (url === '/api/v1/telemetry') return Promise.resolve(respond(200, TELEMETRY));
    if (url === '/health') {
      return Promise.resolve(
        respond(200, { status, version: '0.2.20', uptime_seconds: 1 }),
      );
    }
    throw new Error(`unexpected ${url}`);
  });
  vi.stubGlobal('fetch', fetchMock);
  const store = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => store.set(key, value),
  });
  const dom = mount(<Upstreams route={ROUTE} />);
  await flushPage();
  return dom;
}

describe('naming the strategy', () => {
  it('puts it in the subtitle when `/config` answered', async () => {
    const dom = await mountPage({ dns: { upstreams: { strategy: 'adaptive' } } });
    expect(dom.querySelector('.sub')?.textContent).toContain('strategy adaptive');
  });

  it('names `fallback` just as plainly', async () => {
    const dom = await mountPage({ dns: { upstreams: { strategy: 'fallback' } } });
    expect(dom.querySelector('.sub')?.textContent).toContain('strategy fallback');
  });

  it('says it could not read one, and still renders the counters', async () => {
    const dom = await mountPage(null);
    expect(dom.querySelector('.sub')?.textContent).toContain(
      'strategy unknown — configuration unreachable',
    );
    expect(dom.textContent).toContain('201,883');
    expect(dom.querySelectorAll('.ep')).toHaveLength(2);
  });
});

describe('the page’s banner and footer', () => {
  it('renders no banner while health is ok', async () => {
    const dom = await mountPage({ dns: { upstreams: { strategy: 'adaptive' } } });
    expect(dom.querySelector('.banner')).toBeNull();
  });

  it('renders the amber banner, not an alarm, while health is degraded', async () => {
    const dom = await mountPage(
      { dns: { upstreams: { strategy: 'adaptive' } } },
      'degraded',
    );
    const banner = dom.querySelector('.banner');
    expect(banner?.className).toContain('warn');
    expect(banner?.className).not.toContain('bad');
    expect(banner?.textContent).toContain('this is not an outage');
  });

  it('explains the unknown family in the footer when a row has one', async () => {
    const dom = await mountPage({ dns: { upstreams: { strategy: 'adaptive' } } });
    expect(dom.querySelector('.ep-foot')?.textContent).toContain(
      'resolved at connect time',
    );
  });

  it('leaves that sentence out when every family is known', async () => {
    const known = {
      upstreams: [endpoint(), endpoint({ address: '9.9.9.9:853' })],
    } as unknown as Telemetry;
    const fetchMock = vi.fn((url: string) => {
      if (url === '/api/v1/config') {
        return Promise.resolve(
          respond(200, { dns: { upstreams: { strategy: 'adaptive' } } }),
        );
      }
      if (url === '/api/v1/telemetry') return Promise.resolve(respond(200, known));
      return Promise.resolve(
        respond(200, { status: 'ok', version: '0.2.20', uptime_seconds: 1 }),
      );
    });
    vi.stubGlobal('fetch', fetchMock);
    const store = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => store.set(key, value),
    });
    const dom = mount(<Upstreams route={ROUTE} />);
    await flushPage();
    expect(dom.querySelector('.ep-foot')?.textContent).not.toContain(
      'resolved at connect time',
    );
  });
});

describe('the round-trip cells on an endpoint row', () => {
  it('prints the percentiles and the exact mean in milliseconds', () => {
    const dom = mount(
      <EndpointRow
        index={0}
        upstream={endpoint({
          rtt: { count: 1000, sum_seconds: 12.5, p50: 0.01, p99: 0.05 },
        })}
        mode="adaptive"
      />,
    );
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(dom.querySelectorAll('.ep-rtt-cells > div')).toHaveLength(4);
    expect(text).toContain('10.0 ms');
    expect(text).toContain('50.0 ms');
    // 12.5 s over 1000 answers is 12.5 ms, the one exact figure in the group.
    expect(text).toContain('12.5 ms');
    expect(text).toContain('1,000');
    expect(text).toContain('answered attempts only');
  });

  // An engine predating the field serves no `rtt` at all, and a fresh one that
  // has forwarded nothing serves zeros. Neither is a round trip of zero.
  it('prints nothing rather than a zero when there is no measurement', () => {
    const absent = mount(
      <EndpointRow index={0} upstream={endpoint()} mode="adaptive" />,
    );
    expect(
      [...absent.querySelectorAll('.ep-rtt-cells > div')].every((cell) =>
        (cell.textContent ?? '').includes('—'),
      ),
    ).toBe(true);
  });

  it('reads an exact 0.0 percentile as no traffic, not as an instant answer', () => {
    const dom = mount(
      <EndpointRow
        index={0}
        upstream={endpoint({
          rtt: { count: 0, sum_seconds: 0, p50: 0, p99: 0 },
        })}
        mode="adaptive"
      />,
    );
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).not.toContain('0 ms');
    expect(text).toContain('—');
  });

  it('keeps the round-trip group out of the counter grid', () => {
    // The health cells are gated on the strategy; round trip is not, so a
    // fallback row still carries it and the counter count stays four.
    const dom = mount(
      <EndpointRow
        index={0}
        upstream={endpoint({
          rtt: { count: 10, sum_seconds: 0.1, p50: 0.01, p99: 0.01 },
        })}
        mode="fallback"
      />,
    );
    expect(dom.querySelectorAll('.ep-counters > div')).toHaveLength(4);
    expect(dom.querySelectorAll('.ep-rtt-cells > div')).toHaveLength(4);
  });
});

describe('the endpoints the round-trip chart draws', () => {
  const row = (ts: string, addresses: string[]) => ({
    ts,
    upstreams: addresses.map((address) => endpoint({ address })),
  });

  it('takes the newest row that carries endpoints, not the oldest', () => {
    // A config reload mid-range changes the set; the newest row is the one
    // whose endpoints still exist.
    expect(
      endpointOrder([
        row('2026-08-01T00:00:00Z', ['1.1.1.1']),
        row('2026-08-01T00:01:00Z', ['9.9.9.9', '8.8.8.8']),
      ]),
    ).toEqual(['9.9.9.9', '8.8.8.8']);
  });

  it('skips rows that carry no endpoints rather than reporting none', () => {
    expect(
      endpointOrder([
        row('2026-08-01T00:00:00Z', ['1.1.1.1']),
        { ts: '2026-08-01T00:01:00Z' },
      ]),
    ).toEqual(['1.1.1.1']);
  });

  it('reports none only when no row carries an endpoint', () => {
    expect(endpointOrder([{ ts: '2026-08-01T00:00:00Z' }])).toEqual([]);
    expect(endpointOrder([])).toEqual([]);
  });

  it('caps the plot at four endpoints, where the cards above carry them all', () => {
    expect(
      endpointOrder([
        row('2026-08-01T00:00:00Z', ['a', 'b', 'c', 'd', 'e', 'f']),
      ]),
    ).toEqual(['a', 'b', 'c', 'd']);
  });
});
