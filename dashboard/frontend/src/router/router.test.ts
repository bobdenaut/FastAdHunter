import { beforeEach, describe, expect, it, vi } from 'vitest';
import { currentPath, isPlainLeftClick, navigate, normalize, subscribeRoute } from './router';

interface HistoryCall {
  path: string;
  replace: boolean;
}

const calls: HistoryCall[] = [];
let path = '/';
const windowListeners = new Map<string, Set<() => void>>();

beforeEach(() => {
  calls.length = 0;
  path = '/';
  windowListeners.clear();
  vi.stubGlobal('window', {
    location: {
      get pathname() {
        return path;
      },
    },
    history: {
      pushState: (_s: unknown, _t: string, next: string) => {
        path = next;
        calls.push({ path: next, replace: false });
      },
      replaceState: (_s: unknown, _t: string, next: string) => {
        path = next;
        calls.push({ path: next, replace: true });
      },
    },
    scrollTo: vi.fn(),
    addEventListener: (name: string, handler: () => void) => {
      const set = windowListeners.get(name) ?? new Set();
      set.add(handler);
      windowListeners.set(name, set);
    },
    removeEventListener: (name: string, handler: () => void) => {
      windowListeners.get(name)?.delete(handler);
    },
  });
});

describe('normalize', () => {
  it('folds a trailing slash and an empty path', () => {
    expect(normalize('/lists/')).toBe('/lists');
    expect(normalize('/')).toBe('/');
    expect(normalize('')).toBe('/');
    expect(normalize('/diagnostics/health')).toBe('/diagnostics/health');
  });
});

describe('navigate', () => {
  it('pushes and announces', () => {
    const seen: string[] = [];
    const release = subscribeRoute((next) => seen.push(next));
    navigate('/lists');
    release();
    expect(currentPath()).toBe('/lists');
    expect(seen).toEqual(['/lists']);
    expect(calls).toEqual([{ path: '/lists', replace: false }]);
  });

  it('does not push the path it is already on', () => {
    navigate('/');
    expect(calls).toHaveLength(0);
  });

  it('replaces when asked, even onto the current path', () => {
    navigate('/', { replace: true });
    expect(calls).toEqual([{ path: '/', replace: true }]);
  });

  it('resets scroll unless told to keep it', () => {
    navigate('/lists');
    expect(window.scrollTo).toHaveBeenCalledTimes(1);
    navigate('/cache', { keepScroll: true });
    expect(window.scrollTo).toHaveBeenCalledTimes(1);
  });
});

describe('popstate', () => {
  it('is listened to only while something is subscribed', () => {
    const release = subscribeRoute(() => {});
    expect(windowListeners.get('popstate')?.size).toBe(1);
    release();
    expect(windowListeners.get('popstate')?.size).toBe(0);
  });

  it('announces the browser-driven path', () => {
    const seen: string[] = [];
    const release = subscribeRoute((next) => seen.push(next));
    path = '/policies';
    for (const handler of windowListeners.get('popstate') ?? []) handler();
    release();
    expect(seen).toEqual(['/policies']);
  });
});

describe('click interception', () => {
  const event = (over: Partial<MouseEvent>) =>
    ({ button: 0, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, defaultPrevented: false, ...over }) as MouseEvent;

  it('takes a plain left-click only', () => {
    expect(isPlainLeftClick(event({}))).toBe(true);
    expect(isPlainLeftClick(event({ button: 1 }))).toBe(false);
    expect(isPlainLeftClick(event({ metaKey: true }))).toBe(false);
    expect(isPlainLeftClick(event({ ctrlKey: true }))).toBe(false);
    expect(isPlainLeftClick(event({ shiftKey: true }))).toBe(false);
    expect(isPlainLeftClick(event({ altKey: true }))).toBe(false);
    expect(isPlainLeftClick(event({ defaultPrevented: true }))).toBe(false);
  });
});
