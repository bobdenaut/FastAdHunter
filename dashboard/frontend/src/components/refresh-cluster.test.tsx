// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AGE_TICK_MS } from '../constants';
import { ageTickerRunning } from '../lifecycle/timers';
import { RefreshRegistry } from '../refresh/registry';
import type { RefreshEndpoint } from '../router/routes';
import { DataAge, formatAge } from './data-age';
import { RefreshCluster } from './refresh-cluster';

const store = new Map<string, string>();
let host: HTMLElement | null = null;

function counting(): { registry: RefreshRegistry; calls: Record<string, number> } {
  const calls: Record<string, number> = { health: 0, telemetry: 0, cache: 0 };
  const fetcher = (name: RefreshEndpoint) => () => {
    calls[name] = (calls[name] ?? 0) + 1;
    return Promise.resolve({});
  };
  return {
    registry: new RefreshRegistry({
      health: fetcher('health'),
      telemetry: fetcher('telemetry'),
      cache: fetcher('cache'),
    }),
    calls,
  };
}

function mount(node: preact.ComponentChild): HTMLElement {
  host = document.createElement('div');
  document.body.append(host);
  act(() => {
    render(node, host as HTMLElement);
  });
  return host;
}

beforeEach(() => {
  store.clear();
  vi.useFakeTimers();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => store.set(key, value),
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
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('data age', () => {
  it('reads in seconds, minutes and hours', () => {
    const now = 1_000_000;
    expect(formatAge(null, now)).toBe('not read yet');
    expect(formatAge(now, now)).toBe('0 s ago');
    expect(formatAge(now - 12_000, now)).toBe('12 s ago');
    expect(formatAge(now - 120_000, now)).toBe('2 m ago');
    expect(formatAge(now - 4 * 3_600_000, now)).toBe('4 h ago');
  });

  it('never reads as being from the future', () => {
    expect(formatAge(2_000, 1_000)).toBe('0 s ago');
  });

  it('keeps the ticker alive only while one is mounted', () => {
    expect(ageTickerRunning()).toBe(false);
    const el = mount(<DataAge fetchedAt={Date.now()} />);
    expect(ageTickerRunning()).toBe(true);
    act(() => {
      render(null, el);
    });
    expect(ageTickerRunning()).toBe(false);
  });

  it('advances without a reload', () => {
    const start = Date.now();
    const el = mount(<DataAge fetchedAt={start - 5_000} />);
    expect(el.textContent).toBe('5 s ago');
    act(() => {
      vi.advanceTimersByTime(AGE_TICK_MS);
    });
    expect(el.textContent).toBe('35 s ago');
  });

  it('says "updated" only where the artboards do', () => {
    const el = mount(<DataAge fetchedAt={Date.now()} prefix />);
    expect(el.textContent).toBe('updated 0 s ago');
  });
});

describe('the refresh cluster', () => {
  it('draws age, selector and Refresh, in that order', () => {
    const h = counting();
    const el = mount(
      <RefreshCluster registry={h.registry} endpoint="cache" />,
    );
    const cluster = el.querySelector('.ctl');
    expect([...(cluster?.children ?? [])].map((n) => n.className)).toEqual([
      'age',
      'sel',
      'mini',
      'visually-hidden',
    ]);
    h.registry.dispose();
  });

  it('offers exactly that endpoint’s options, labelled as the artboards do', () => {
    const h = counting();
    const el = mount(
      <RefreshCluster registry={h.registry} endpoint="health" />,
    );
    expect(
      [...el.querySelectorAll('option')].map((n) => n.textContent),
    ).toEqual(['30 s', '1 m', '5 m']);
    h.registry.dispose();
  });

  it('subscribes the widget without starting a second request', () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    expect(h.calls['cache']).toBe(1);
    mount(<RefreshCluster registry={h.registry} endpoint="cache" />);
    expect(h.calls['cache']).toBe(1);
    h.registry.dispose();
  });

  it('issues no request when the interval changes', () => {
    const h = counting();
    h.registry.subscribe('telemetry', () => {});
    const el = mount(
      <RefreshCluster registry={h.registry} endpoint="telemetry" />,
    );
    const before = h.calls['telemetry'];

    const select = el.querySelector('select') as HTMLSelectElement;
    select.value = '60';
    act(() => {
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });

    expect(h.calls['telemetry']).toBe(before);
    expect(store.get('fah-refresh-telemetry')).toBe('60');
    h.registry.dispose();
  });

  it('keeps two clusters for one endpoint showing the same value', () => {
    const h = counting();
    const el = mount(
      <div>
        <RefreshCluster registry={h.registry} endpoint="telemetry" />
        <RefreshCluster registry={h.registry} endpoint="telemetry" />
      </div>,
    );
    const [first, second] = [...el.querySelectorAll('select')];
    (first as HTMLSelectElement).value = '60';
    act(() => {
      first?.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect((second as HTMLSelectElement).value).toBe('60');
    h.registry.dispose();
  });

  it('refreshes on demand, and one click is one request', async () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    await vi.advanceTimersByTimeAsync(0);
    const el = mount(
      <RefreshCluster registry={h.registry} endpoint="cache" />,
    );
    const button = el.querySelector('button') as HTMLButtonElement;
    act(() => {
      button.click();
    });
    expect(h.calls['cache']).toBe(2);
    h.registry.dispose();
  });

  it('coalesces rapid clicks into the in-flight request', async () => {
    const h = counting();
    h.registry.subscribe('cache', () => {});
    await vi.advanceTimersByTimeAsync(0);
    const el = mount(
      <RefreshCluster registry={h.registry} endpoint="cache" />,
    );
    const button = el.querySelector('button') as HTMLButtonElement;
    act(() => {
      button.click();
      button.click();
      button.click();
    });
    expect(h.calls['cache']).toBe(2);
    h.registry.dispose();
  });

  it('takes the mobile row class off the header placement', () => {
    const h = counting();
    const el = mount(
      <RefreshCluster
        registry={h.registry}
        endpoint="cache"
        placement="header"
      />,
    );
    expect(el.querySelector('.ctl')?.className).toBe('ctl');
    h.registry.dispose();
  });
});
