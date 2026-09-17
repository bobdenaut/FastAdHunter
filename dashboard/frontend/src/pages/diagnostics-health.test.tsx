// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ListsResponse, Telemetry, Upstream } from '../api/types';
import type { Route } from '../router/routes';
import DiagnosticsHealth from './diagnostics-health';
import { EndpointSummary } from './health/endpoint-summary';
import { EngineCard } from './health/engine-card';
import { RuleListsCard } from './health/rule-lists-card';

/**
 * Every figure on this page traces to `/health`, `/telemetry`, `/lists` or the
 * one `/config` strategy read. The two that could be got wrong are the endpoint
 * summary — which must not count states it cannot attribute to a strategy — and
 * the memory pair, which is `null` off Linux and must never read as zero.
 */

const ROUTE: Route = {
  path: '/diagnostics/health',
  title: 'Health',
  section: 'system',
  group: 'diagnostics',
  events: [],
  endpoints: ['health', 'telemetry', 'lists'],
  built: true,
  ownsHeader: true,
  load: null,
};

function endpoint(over: Partial<Upstream> = {}): Upstream {
  return {
    address: '1.1.1.1:853',
    protocol: 'dot',
    attempts: 10,
    failures: 0,
    consecutive_failures: 0,
    tls_handshakes: 1,
    failure_runs: [0, 0, 0, 0],
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

const TELEMETRY = {
  process: { version: '0.2.20', uptime_seconds: 16_260 },
  ruleset: {
    rules: 752_585,
    duplicates_removed: 87_422,
    compile_duration_seconds: 7.41,
  },
  counters: {
    dns: {
      pass: 0,
      allow: 0,
      block: 0,
      cache_hits: 0,
      cache_misses: 0,
      cache_stale: 0,
      answers: {
        servfail_synthesized: 1204,
        servfail_relayed: 88,
        refused_relayed: 17,
      },
    },
    http: {
      pass: 0,
      allow: 0,
      block: 0,
      response_bytes: 0,
      refused_claim: 3,
      refused_destination: 0,
    },
    events_dropped: 0,
    swr: { enqueued: 0, deduplicated: 0, dropped: 0, completed: 0, failed: 31 },
    cache_cleanup: {
      runs: 0,
      entries_removed: 0,
      bytes_freed: 0,
      last_duration_micros: 0,
    },
    lists: { bodies: 17, not_modified: 3, bytes_fetched: 27_580_000 },
    dns_tcp_connections: { active: 2, peak: 9, closed_oversize: 0 },
    dns_dot_connections: { active: 1, peak: 6, closed_oversize: 0 },
    dns_udp_inflight: { active: 0, peak: 0, shed: 4 },
    tasks_died: 0,
  },
  upstreams: [
    endpoint(),
    endpoint({ address: '9.9.9.9:853', state: 'penalized' }),
    endpoint({ address: '8.8.8.8:853', state: 'probing' }),
  ],
  memory: {
    process_rss: 57_567_641,
    process_peak_rss: 140_194_611,
  },
} as unknown as Telemetry;

const LISTS: ListsResponse = {
  compiled_rules: 752_585,
  duplicates_removed: 87_422,
  items: [
    {
      id: 'oisd-basic',
      url: 'x',
      format: 'domains',
      enabled: true,
      refresh_hours: 24,
      last_refresh: null,
      last_status: 'ok',
      rules_total: 1,
      rules_active_dns: 1,
      rules_active_url: 0,
      rules_inactive: 0,
      parse_errors: 0,
    },
    {
      id: 'hagezi-pro',
      url: 'x',
      format: 'domains',
      enabled: true,
      refresh_hours: 24,
      last_refresh: null,
      last_status: 'failed',
      rules_total: 1,
      rules_active_dns: 1,
      rules_active_url: 0,
      rules_inactive: 0,
      parse_errors: 0,
      last_error: 'fetch timed out',
    },
    {
      id: 'adaway',
      url: 'x',
      format: 'hosts',
      enabled: true,
      refresh_hours: 24,
      last_refresh: null,
      last_status: 'rejected',
      rules_total: 0,
      rules_active_dns: 0,
      rules_active_url: 0,
      rules_inactive: 0,
      parse_errors: 0,
      last_error: 'collapse: 0 dns rules vs baseline 6,710',
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

let host: HTMLElement | null = null;

function mount(node: preact.ComponentChild): HTMLElement {
  host = document.createElement('div');
  document.body.append(host);
  act(() => {
    render(node, host as HTMLElement);
  });
  return host;
}

async function flush(): Promise<void> {
  await act(async () => {
    for (let turn = 0; turn < 12; turn += 1) await Promise.resolve();
  });
}

/** `strategy: null` makes `GET /config` fail, which is the unknown case. */
async function mountPage(
  strategy: string | null,
  status: 'ok' | 'degraded' = 'ok',
): Promise<HTMLElement> {
  const fetchMock = vi.fn((url: string) => {
    if (url === '/api/v1/config') {
      return Promise.resolve(
        strategy === null
          ? respond(500, { error: { code: 'internal', message: 'no' } })
          : respond(200, { dns: { upstreams: { strategy } } }),
      );
    }
    if (url === '/api/v1/telemetry') {
      return Promise.resolve(respond(200, TELEMETRY));
    }
    if (url === '/api/v1/lists') return Promise.resolve(respond(200, LISTS));
    if (url === '/health') {
      return Promise.resolve(
        respond(200, { status, version: '0.2.20', uptime_seconds: 16_260 }),
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
  const dom = mount(<DiagnosticsHealth route={ROUTE} />);
  await flush();
  return dom;
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

describe('the endpoint summary', () => {
  it('counts the three real states under `adaptive`', () => {
    const dom = mount(
      <EndpointSummary mode="adaptive" upstreams={TELEMETRY.upstreams} />,
    );
    expect(dom.textContent).toContain(
      '3 endpoints — 1 healthy, 1 penalized, 1 probing',
    );
  });

  it('invents no fourth state', () => {
    // The artboard draws "0 healthy, 1 penalized, 1 probing, 1 recovering".
    // `UpstreamState` has three values and the counts must sum to the total.
    const dom = mount(
      <EndpointSummary mode="adaptive" upstreams={TELEMETRY.upstreams} />,
    );
    expect(dom.textContent).not.toContain('recovering');
  });

  it('says the strategy could not be read when it could not', () => {
    const dom = mount(
      <EndpointSummary mode="unknown" upstreams={TELEMETRY.upstreams} />,
    );
    expect(dom.textContent).toContain('3 endpoints');
    expect(dom.textContent).not.toContain('penalized');
    expect(dom.textContent).toContain('could not be read');
  });
});

describe('the rule-lists summary', () => {
  it('counts only failed and rejected as needing attention', () => {
    const dom = mount(<RuleListsCard lists={LISTS} counters={null} />);
    expect(dom.textContent).toContain('2 of 3 need attention');
    expect(dom.textContent).toContain('fetch timed out');
    expect(dom.textContent).toContain('1 other is ok');
  });

  it('says so plainly when nothing is wrong', () => {
    const dom = mount(
      <RuleListsCard
        lists={{ ...LISTS, items: [LISTS.items[0]!] }}
        counters={null}
      />,
    );
    expect(dom.textContent).toContain('0 of 1 need attention');
    expect(dom.textContent).toContain('refreshing normally');
  });

  it('states that neither state is an outage', () => {
    const dom = mount(<RuleListsCard lists={LISTS} counters={null} />);
    expect(dom.textContent).toContain('Neither is an outage');
  });

  it('renders the three refresh counters, bytes formatted', () => {
    const dom = mount(
      <RuleListsCard lists={LISTS} counters={TELEMETRY.counters.lists} />,
    );
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('17 refreshes downloaded a body');
    expect(text).toContain('26.3 MiB total');
    expect(text).toContain('3 answered 304');
    expect(text).toContain('cost no download, no recompile');
  });

  it('renders no counters line before telemetry is read', () => {
    const dom = mount(<RuleListsCard lists={LISTS} counters={null} />);
    expect(dom.textContent).not.toContain('304');
  });
});

describe('the engine card', () => {
  it('renders the ruleset figures verbatim', () => {
    const dom = mount(<EngineCard telemetry={TELEMETRY} />);
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('752,585');
    expect(text).toContain('87,422');
    expect(text).toContain('7.41 s');
  });

  it('renders the supervised-task death count, red only when non-zero', () => {
    const quiet = mount(<EngineCard telemetry={TELEMETRY} />);
    expect(quiet.textContent).toContain('supervised tasks died');
    expect(quiet.querySelector('.figure.bad')).toBeNull();

    const wounded = {
      ...TELEMETRY,
      counters: { ...TELEMETRY.counters, tasks_died: 2 },
    };
    const dom = mount(<EngineCard telemetry={wounded} />);
    expect(dom.querySelector('.figure.bad')?.textContent).toBe('2');
  });

  it('renders a null memory reading as unavailable, never as zero', () => {
    const offLinux = {
      ...TELEMETRY,
      memory: { process_rss: null, process_peak_rss: null },
    } as unknown as Telemetry;
    const dom = mount(<EngineCard telemetry={offLinux} />);
    expect(dom.textContent).toContain('unavailable');
    expect(dom.textContent).not.toContain('0 MiB');
  });
});

describe('the page', () => {
  it('carries one refresh cluster per polled endpoint, and no more', () => {
    // Three declared endpoints, three clusters: health on the status card,
    // telemetry on the outcomes card, lists on the rule-lists card.
    return mountPage('adaptive').then((dom) => {
      expect(dom.querySelectorAll('.ctl')).toHaveLength(3);
      const labels = [...dom.querySelectorAll('.ctl select')].map((node) =>
        node.getAttribute('aria-label'),
      );
      expect(labels).toEqual([
        'Refresh interval for health',
        'Refresh interval for telemetry',
        'Refresh interval for lists',
      ]);
    });
  });

  it('explains `degraded` with the shared module rather than a copy', async () => {
    const dom = await mountPage('adaptive', 'degraded');
    const banner = dom.querySelector('.banner.warn');
    expect(banner?.textContent).toContain('this is not an outage');
    expect(banner?.textContent).toContain('every one is');
  });

  it('draws no banner while the status is ok', async () => {
    const dom = await mountPage('adaptive');
    expect(dom.querySelector('.banner')).toBeNull();
  });

  it('prints the uptime and the version from `/health`', async () => {
    const dom = await mountPage('adaptive');
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('4h 31m');
    expect(text).toContain('v0.2.20');
  });

  it('renders the backpressure counters verbatim', async () => {
    const dom = await mountPage('adaptive');
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('events dropped — shed, both pipelines');
    expect(text).toContain('HTTP requests refused — unusable Host3');
    expect(text).toContain('HTTP requests refused — egress policy0');
    expect(text).toContain('SWR refreshes failed');
  });

  it('renders the three DNS listener gauges as active / peak, and the shed count', async () => {
    const dom = await mountPage('adaptive');
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('DNS-over-TCP connections — active / peak2 / 9');
    expect(text).toContain('DoT connections — active / peak1 / 6');
    expect(text).toContain('UDP queries in flight — active / peak0 / 0');
    expect(text).toContain('UDP datagrams shed — in-flight ceiling full4');
  });

  it('states the three things it will never do', async () => {
    const dom = await mountPage('adaptive');
    const text = dom.querySelector('.health-never')?.textContent ?? '';
    expect(text).toContain('No message store');
    expect(text).toContain('No log viewer');
    expect(text).toContain('No red without a reason');
  });

  it('falls back to a count with no states when `/config` failed', async () => {
    const dom = await mountPage(null);
    expect(dom.textContent).toContain('the strategy could not be read');
    // The counters still render: a failed `/config` costs the strategy alone.
    expect(dom.textContent).toContain('1,204');
  });
});
