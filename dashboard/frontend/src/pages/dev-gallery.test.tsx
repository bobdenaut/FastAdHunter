// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEV_GALLERY_MARKER } from '../constants';
import { GALLERY_ROUTE } from '../router/routes';
import { DevGallery } from './dev-gallery';

const store = new Map<string, string>();
let host: HTMLElement | null = null;

beforeEach(() => {
  store.clear();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => store.set(key, value),
  });
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
    ok: true,
    status: 200,
    headers: { get: () => null },
    json: async () => ({}),
  }));
  vi.stubGlobal('ResizeObserver', class {
    observe(): void {}
    disconnect(): void {}
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

describe('the dev gallery', () => {
  it('renders every vocabulary component', () => {
    host = document.createElement('div');
    document.body.append(host);
    act(() => {
      render(<DevGallery route={GALLERY_ROUTE} />, host as HTMLElement);
    });

    expect(host.querySelectorAll('.tile')).toHaveLength(4);
    expect(host.querySelectorAll('.card').length).toBeGreaterThanOrEqual(6);
    expect(host.querySelector('table.t')).not.toBeNull();
    expect(host.querySelectorAll('.pill').length).toBeGreaterThanOrEqual(9);
    expect(host.querySelector('.seg')).not.toBeNull();
    expect(host.querySelector('.empty-state')).not.toBeNull();
    expect(host.querySelector('.error-state')).not.toBeNull();
    expect(host.querySelector('.chart')).not.toBeNull();
  });

  it('carries the marker the postbuild grep looks for', () => {
    host = document.createElement('div');
    document.body.append(host);
    act(() => {
      render(<DevGallery route={GALLERY_ROUTE} />, host as HTMLElement);
    });
    expect(host.querySelector(`[data-marker="${DEV_GALLERY_MARKER}"]`)).not.toBeNull();
  });

  it('draws one cluster for /cache even though two cards read it', () => {
    host = document.createElement('div');
    document.body.append(host);
    act(() => {
      render(<DevGallery route={GALLERY_ROUTE} />, host as HTMLElement);
    });
    const labels = [...host.querySelectorAll('select')].map((node) =>
      node.getAttribute('aria-label'),
    );
    expect(labels).toEqual([
      'Refresh interval for cache',
      'Refresh interval for telemetry',
    ]);
  });

  it('declares exactly the events and endpoints it renders', () => {
    expect(GALLERY_ROUTE.events).toEqual(['stats']);
    expect(GALLERY_ROUTE.endpoints).toEqual(['telemetry', 'cache']);
  });
});
