import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  blockNavigation,
  currentPath,
  isPlainLeftClick,
  navigate,
  navigationBlocked,
  normalize,
  subscribeRoute,
} from './router';

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

  // F2 — Back/Forward is in-app navigation too: while a recompiling mutation
  // holds the block, a popstate must neither announce (which would unmount the
  // page) nor leave the browser on the entry it moved to.
  it('restores the held path and announces nothing while navigation is blocked', () => {
    const seen: string[] = [];
    const release = subscribeRoute((next) => seen.push(next));
    navigate('/rules');
    const unblock = blockNavigation();

    path = '/clients';
    for (const handler of windowListeners.get('popstate') ?? []) handler();

    expect(seen).toEqual(['/rules']);
    expect(path).toBe('/rules');
    expect(calls[calls.length - 1]).toEqual({ path: '/rules', replace: false });
    expect(navigationBlocked()).toBe(true);

    unblock();
    path = '/clients';
    for (const handler of windowListeners.get('popstate') ?? []) handler();
    expect(seen).toEqual(['/rules', '/clients']);
    release();
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
