// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { HistoryPerf, PerfItem } from '../api/types';
import type { Route } from '../router/routes';
import Performance from './performance';
import { StageTiles } from './performance/stage-tiles';

/**
 * `Performance.dc.html` and the task decide what may be rendered: per-stage
 * percentiles only, no average anywhere, budgets as markers, and a disabled
 * recorder as its own state rather than as an empty range.
 *
 * The plots themselves are not constructed here — uPlot needs a canvas 2D
 * context jsdom does not implement, and the wrapper's dynamic import never
 * resolves in this environment. Everything asserted below is markup around the
 * plot, or a branch that draws no plot at all.
 */

const ROUTE: Route = {
  path: '/performance',
  title: 'Performance',
  section: 'runtime',
  events: [],
  endpoints: [],
  built: true,
  ownsHeader: true,
  load: null,
};

function sample(over: Partial<PerfItem> = {}): PerfItem {
  return {
    ts: '2026-08-27T10:00:00Z',
    qps: 12.5,
    queries_delta: 750,
    blocked_delta: 210,
    allowed_delta: 5,
    latency: {
      block_p50: 0.000_02,
      block_p99: 0.000_039,
      cache_hit_p50: 0.000_03,
      cache_hit_p99: 0.000_051,
      forward_p50: 0.000_2,
      forward_p99: 0.000_412,
    },
    ...over,
  };
}

const HISTORY: HistoryPerf = {
  from: '2026-08-26T10:00:00Z',
  to: '2026-08-27T10:00:00Z',
  stride: 1,
  items: [sample({ ts: '2026-08-26T10:00:00Z', qps: 28.4 }), sample()],
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

function calls(): string[] {
  return fetchMock.mock.calls.map((call) => call[0] as string);
}

function perfCalls(): string[] {
  return calls().filter((url) => url.startsWith('/api/v1/history/perf'));
}

function configCalls(): string[] {
  return calls().filter((url) => url === '/api/v1/config');
}

/** Drains microtasks and then a macrotask, several times over: the page's two
 *  entry reads are separate round trips and the assertions below are about what
 *  it does once both have landed, not about microtask ordering. */
async function flush(): Promise<void> {
  await act(async () => {
    for (let round = 0; round < 4; round += 1) {
      for (let turn = 0; turn < 8; turn += 1) await Promise.resolve();
      await new Promise((resolve) => {
        setTimeout(resolve, 0);
      });
    }
  });
}

function mount(node: preact.ComponentChild): HTMLElement {
  host = document.createElement('div');
  document.body.append(host);
  act(() => {
    render(node, host as HTMLElement);
  });
  return host;
}

async function mountPage(): Promise<HTMLElement> {
  const dom = mount(<Performance route={ROUTE} />);
  await flush();
  return dom;
}

function unmount(): void {
  if (host === null) return;
  act(() => {
    render(null, host as HTMLElement);
  });
  host.remove();
  host = null;
}

/** A response one macrotask out, so two requests issued together do not settle
 *  in the same microtask queue and the mount `/config` answers first — the
 *  ordinary case, against which the join case is staged explicitly below. */
function later(response: Response): Promise<Response> {
  return new Promise((resolve) => {
    setTimeout(() => resolve(response), 0);
  });
}

/** Answers `/config` with the given body and `/history/perf` with `history`. */
function serve(config: unknown, history: HistoryPerf | null = HISTORY): void {
  fetchMock.mockImplementation((url: string) => {
    if (url === '/api/v1/config') return Promise.resolve(respond(200, config));
    if (url.startsWith('/api/v1/history/perf')) {
      return later(respond(200, history ?? { ...HISTORY, items: [], stride: 1 }));
    }
    throw new Error(`unexpected ${url}`);
  });
}

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
});

afterEach(() => {
  unmount();
  vi.unstubAllGlobals();
});

describe('request discipline', () => {
  it('issues one `/config` and one perf read on entry, and nothing else', async () => {
    serve({ history: { enabled: true, sample_interval_seconds: 60 } });
    await mountPage();
    expect(configCalls()).toHaveLength(1);
    expect(perfCalls()).toHaveLength(1);
    expect(calls()).toHaveLength(2);
  });

  it('asks for exactly the five fields the page draws', async () => {
    serve({ history: { enabled: true } });
    await mountPage();
    const url = perfCalls()[0] ?? '';
    expect(url).toContain(
      'fields=qps%2Cqueries_delta%2Cblocked_delta%2Callowed_delta%2Clatency',
    );
    // Nothing memory-shaped, and no `max_points` override.
    for (const absent of ['rss', 'peak', 'memory', 'upstreams', 'max_points']) {
      expect(url).not.toContain(absent);
    }
  });

  it('issues exactly one more perf read per range change, and no `/config`', async () => {
    serve({ history: { enabled: true } });
    const dom = await mountPage();
    const chip = [...dom.querySelectorAll('.ch .chip')].find(
      (node) => node.textContent === '7 d',
    ) as HTMLButtonElement;
    await act(async () => {
      chip.click();
    });
    await flush();
    expect(perfCalls()).toHaveLength(2);
    expect(configCalls()).toHaveLength(1);
  });
});

describe('the recorder’s two empty answers', () => {
  it('renders its own state when `/config` says recording is off', async () => {
    serve({ history: { enabled: false } }, { ...HISTORY, items: [] });
    const dom = await mountPage();
    expect(dom.textContent).toContain('History is not being recorded');
    // No chart cards at all, and no range chips to press.
    expect(dom.querySelector('.chips')).toBeNull();
    expect(dom.querySelector('.chart')).toBeNull();
    // The explainer survives: it explains the page, not the data.
    expect(dom.textContent).toContain('Three stages, never one number');
  });

  it('renders the per-card empty state when an enabled recorder has no rows', async () => {
    serve({ history: { enabled: true } }, { ...HISTORY, items: [] });
    const dom = await mountPage();
    expect(dom.textContent).toContain('No data in this range');
    expect(dom.textContent).not.toContain('History is not being recorded');
    // One disambiguating re-read, and exactly one.
    expect(configCalls()).toHaveLength(2);
  });

  it('joins the mount read rather than issuing a second concurrent `/config`', async () => {
    // The empty answer beats the mount read home. Two concurrent reads of the
    // same document is p5-06's F11, and this is the page that inherited it.
    const configResolvers: Array<(value: Response) => void> = [];
    fetchMock.mockImplementation((url: string) => {
      if (url === '/api/v1/config') {
        return new Promise<Response>((resolve) => {
          configResolvers.push(resolve);
        });
      }
      return Promise.resolve(respond(200, { ...HISTORY, items: [] }));
    });

    mount(<Performance route={ROUTE} />);
    await flush();
    expect(configCalls()).toHaveLength(1);

    await act(async () => {
      configResolvers[0]?.(respond(200, { history: { enabled: true } }));
    });
    await flush();
    // Still one: the disambiguation joined the in-flight read.
    expect(configCalls()).toHaveLength(1);
  });
});

describe('failure paths', () => {
  it('renders the API’s own message when the perf read fails', async () => {
    fetchMock.mockImplementation((url: string) => {
      if (url === '/api/v1/config') {
        return Promise.resolve(respond(200, { history: { enabled: true } }));
      }
      return Promise.resolve(
        respond(503, {
          error: { code: 'unavailable', message: 'history is not available' },
        }),
      );
    });
    const dom = await mountPage();
    expect(dom.querySelector('.error-state-message')?.textContent).toBe(
      'history is not available',
    );
    // The chips stay live, so a retry is a range re-selection.
    expect(dom.querySelectorAll('.ch .chip')).toHaveLength(3);
  });

  it('draws the charts and falls back to 60 s when `/config` fails', async () => {
    fetchMock.mockImplementation((url: string) => {
      if (url === '/api/v1/config') {
        return Promise.resolve(
          respond(500, { error: { code: 'internal', message: 'no' } }),
        );
      }
      return Promise.resolve(respond(200, HISTORY));
    });
    const dom = await mountPage();
    expect(dom.querySelector('.sub')?.textContent).toContain('one row per 60 s');
    expect(dom.textContent).not.toContain('History is not being recorded');
  });

  it('takes the sampling interval from `/config` when it answered', async () => {
    serve({ history: { enabled: true, sample_interval_seconds: 30 } });
    const dom = await mountPage();
    expect(dom.querySelector('.sub')?.textContent).toContain('one row per 30 s');
  });

  it('settles into the empty states when the disambiguating re-read fails', async () => {
    let configs = 0;
    fetchMock.mockImplementation((url: string) => {
      if (url === '/api/v1/config') {
        configs += 1;
        return configs === 1
          ? Promise.resolve(respond(200, { history: { enabled: true } }))
          : Promise.resolve(
              respond(500, { error: { code: 'internal', message: 'no' } }),
            );
      }
      return later(respond(200, { ...HISTORY, items: [] }));
    });
    const dom = await mountPage();
    expect(configs).toBe(2);
    expect(dom.textContent).toContain('No data in this range');
    expect(dom.querySelector('.boot')).toBeNull();
  });
});

describe('what the page states about its own figures', () => {
  it('shows the decimation footnote when the response was thinned', async () => {
    serve({ history: { enabled: true } }, { ...HISTORY, stride: 44 });
    const dom = await mountPage();
    expect(dom.textContent).toContain('one point every 44 buckets');
  });

  it('labels the real allow verdict `allow`, never `permitted`', async () => {
    serve({ history: { enabled: true } });
    const dom = await mountPage();
    const legends = [...dom.querySelectorAll('.chart-legend')]
      .map((node) => node.textContent ?? '')
      .join(' ');
    expect(legends).toContain('allow');
    expect(legends).not.toContain('permitted');
  });

  it('names no average anywhere', async () => {
    serve({ history: { enabled: true } });
    const dom = await mountPage();
    const text = dom.textContent ?? '';
    // The two places the word appears are the statements that there is none.
    expect(text).toContain('never an average');
    expect(text).toContain('whole rows, never averaged');
    // `meaningless` is allowed — the bare word is not.
    expect(text).not.toMatch(/\bmean\b/);
  });

  it('scopes the QPS stats to the served rows rather than to now', async () => {
    serve({ history: { enabled: true } });
    const dom = await mountPage();
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('latest sample');
    expect(text).toContain('busiest served sample');
    expect(text).toContain('sustained capacity, measured on the RB5009');
    expect(text).not.toContain('now, per second');
  });
});

describe('the stage tiles', () => {
  it('prints each stage’s p99 against its budget chip', () => {
    const dom = mount(<StageTiles error={null} items={[sample()]} />);
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('block stage, p99');
    expect(text).toContain('0.039');
    expect(text).toContain('0.051');
    expect(text).toContain('forward stage, p99 — upstream round trip included');
    expect(text).toContain('0.412');
    expect(dom.querySelectorAll('.stage-budget')).toHaveLength(2);
  });

  // `duration_forward` is timed end to end, upstream round trip included, so
  // the `< 1 ms` row cannot be drawn beside it — a borrowed budget reads as a
  // permanent breach on a tile that is measuring the network.
  it('gives the forward tile no budget chip and no proximity bar', () => {
    const dom = mount(<StageTiles error={null} items={[sample()]} />);
    expect(dom.querySelectorAll('.stage-nobudget')).toHaveLength(1);
    expect(dom.querySelectorAll('.stage-bar-track')).toHaveLength(2);
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('Network time, not engine time');
    expect(text).not.toContain('engine overhead');
  });

  // The stage quantile saturates at the histogram's last finite bucket
  // (0.1 s): a percentile equal to it is a floor, not an exact reading.
  it('prints a saturated percentile as a floor, not as an exact figure', () => {
    const dom = mount(
      <StageTiles error={null}
        items={[
          sample({
            latency: {
              block_p50: 0.000_02,
              block_p99: 0.000_039,
              cache_hit_p50: 0.000_03,
              cache_hit_p99: 0.000_051,
              forward_p50: 0.025,
              forward_p99: 0.1,
            },
          }),
        ]}
      />,
    );
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('≥ 100.000');
    expect(text).toContain('0.039');
    expect(text).not.toContain('≥ 0.039');
  });

  it('keeps the neutral tone for every reading under the budget', () => {
    const dom = mount(<StageTiles error={null} items={[sample()]} />);
    expect(dom.querySelectorAll('.stage-bar-fill.over')).toHaveLength(0);
  });

  it('flips the tone only once a reading has reached the budget', () => {
    const dom = mount(
      <StageTiles error={null}
        items={[
          sample({
            latency: {
              block_p50: 0.000_02,
              block_p99: 0.0012,
              cache_hit_p50: 0.000_03,
              cache_hit_p99: 0.000_051,
              forward_p50: 0.000_2,
              forward_p99: 0.000_412,
            },
          }),
        ]}
      />,
    );
    expect(dom.querySelectorAll('.stage-bar-fill.over')).toHaveLength(1);
  });

  it('reads an exact 0.0 as no traffic rather than as a fast stage', () => {
    const dom = mount(
      <StageTiles error={null}
        items={[
          sample({
            latency: {
              block_p50: 0,
              block_p99: 0,
              cache_hit_p50: 0.000_03,
              cache_hit_p99: 0.000_051,
              forward_p50: 0.000_2,
              forward_p99: 0.000_412,
            },
          }),
        ]}
      />,
    );
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('—');
    expect(text).toContain('No traffic in this stage in the latest sample.');
    expect(text).not.toContain('0.000');
  });

  it('reads the latest **served** row, not the first one', () => {
    const dom = mount(
      <StageTiles error={null}
        items={[
          sample(),
          sample({
            latency: {
              block_p50: 0.000_02,
              block_p99: 0.000_077,
              cache_hit_p50: 0.000_03,
              cache_hit_p99: 0.000_051,
              forward_p50: 0.000_2,
              forward_p99: 0.000_412,
            },
          }),
        ]}
      />,
    );
    expect(dom.textContent).toContain('0.077');
  });

  it('says so rather than printing zeros when the range served nothing', () => {
    const dom = mount(<StageTiles error={null} items={[]} />);
    expect(dom.textContent).toContain('No sample in the selected range.');
  });
});

/**
 * A failed read leaves the previous range's rows in state on purpose — the plot
 * survives a retry rather than blanking. The tiles and the QPS stat row are
 * scoped to the *selected* range, so they are the two places that must not
 * inherit it.
 */
describe('a failed read lends its figures to nothing', () => {
  function texts(dom: HTMLElement, selector: string): string[] {
    return [...dom.querySelectorAll(selector)].map(
      (node) => node.textContent ?? '',
    );
  }

  function servingSecondFailure(): void {
    let perf = 0;
    fetchMock.mockImplementation((url: string) => {
      if (url === '/api/v1/config') {
        return Promise.resolve(respond(200, { history: { enabled: true } }));
      }
      perf += 1;
      return later(
        perf === 1
          ? respond(200, HISTORY)
          : respond(503, {
              error: { code: 'unavailable', message: 'history is not available' },
            }),
      );
    });
  }

  it('says the read failed rather than that the range held no sample', async () => {
    fetchMock.mockImplementation((url: string) =>
      url === '/api/v1/config'
        ? Promise.resolve(respond(200, { history: { enabled: true } }))
        : Promise.resolve(
            respond(503, {
              error: { code: 'unavailable', message: 'history is not available' },
            }),
          ),
    );
    const dom = await mountPage();
    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(text).toContain('The read for this range failed');
    // The two readings are different facts and the page states the true one.
    expect(text).not.toContain('No sample in the selected range.');
    expect(texts(dom, '.stage-figure .big')).toEqual(['—', '—', '—']);
    // Latest and busiest go with the range; the measured-capacity figure is a
    // documentation constant and stays.
    expect(texts(dom, '.stat-figure')).toEqual(['—', '—', '20 k+']);
  });

  it('drops the previous range’s tile figures when the new range fails', async () => {
    servingSecondFailure();
    const dom = await mountPage();
    expect(dom.textContent).toContain('0.039');

    const chip = [...dom.querySelectorAll('.ch .chip')].find(
      (node) => node.textContent === '7 d',
    ) as HTMLButtonElement;
    await act(async () => {
      chip.click();
    });
    await flush();

    const text = (dom.textContent ?? '').replace(/\s+/g, ' ');
    expect(dom.querySelector('.error-state-message')?.textContent).toBe(
      'history is not available',
    );
    expect(texts(dom, '.stage-figure .big')).toEqual(['—', '—', '—']);
    // 24 h's block, cache-hit and forward p99, none of which belong to 7 d.
    for (const stale of ['0.039', '0.051', '0.412']) {
      expect(text).not.toContain(stale);
    }
  });

  it('drops the previous range’s QPS stats when the new range fails', async () => {
    servingSecondFailure();
    const dom = await mountPage();
    expect(texts(dom, '.stat-figure')).toEqual(['12.5', '28.4', '20 k+']);

    const chip = [...dom.querySelectorAll('.ch .chip')].find(
      (node) => node.textContent === '7 d',
    ) as HTMLButtonElement;
    await act(async () => {
      chip.click();
    });
    await flush();

    expect(texts(dom, '.stat-figure')).toEqual(['—', '—', '20 k+']);
  });
});

describe('the budget chip', () => {
  const OVER = sample({
    latency: {
      block_p50: 0.000_02,
      block_p99: 0.0012,
      cache_hit_p50: 0.000_03,
      cache_hit_p99: 0.000_051,
      forward_p50: 0.000_2,
      forward_p99: 0.000_412,
    },
  });

  it('is neutral, so it cannot read as a verdict on the tile', () => {
    const dom = mount(<StageTiles error={null} items={[OVER]} />);
    const chips = [...dom.querySelectorAll('.stage-budget')];
    expect(chips).toHaveLength(2);
    for (const chip of chips) {
      expect(chip.className).toContain('neutral');
      // `good` beside a bar that has flipped to the blocked tone states the
      // opposite of what the bar states.
      expect(chip.className).not.toContain('good');
    }
  });

  it('leaves the bar as the only thing on the tile carrying a tone', () => {
    const dom = mount(<StageTiles error={null} items={[OVER]} />);
    expect(dom.querySelectorAll('.stage-bar-fill.over')).toHaveLength(1);
  });
});
