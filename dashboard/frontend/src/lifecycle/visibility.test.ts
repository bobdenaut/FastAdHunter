import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { isHidden, subscribeVisibility } from './visibility';

const handlers = new Map<string, Set<() => void>>();
let state: DocumentVisibilityState = 'visible';

beforeEach(() => {
  handlers.clear();
  state = 'visible';
  vi.stubGlobal('document', {
    get visibilityState() {
      return state;
    },
    addEventListener: (name: string, handler: () => void) => {
      const set = handlers.get(name) ?? new Set<() => void>();
      set.add(handler);
      handlers.set(name, set);
    },
    removeEventListener: (name: string, handler: () => void) => {
      handlers.get(name)?.delete(handler);
    },
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function fire(): void {
  for (const handler of handlers.get('visibilitychange') ?? []) handler();
}

describe('the visibility signal', () => {
  it('reports the document state', () => {
    expect(isHidden()).toBe(false);
    state = 'hidden';
    expect(isHidden()).toBe(true);
  });

  it('reports each transition to its subscribers', () => {
    const seen: boolean[] = [];
    const release = subscribeVisibility((hidden) => seen.push(hidden));
    state = 'hidden';
    fire();
    state = 'visible';
    fire();
    release();
    expect(seen).toEqual([false, true, false]);
  });

  it('reports the current state at subscribe time', () => {
    state = 'hidden';
    const seen: boolean[] = [];
    // A tab opened or session-restored in the background never fires
    // `visibilitychange` until it is brought forward, which may be never.
    const release = subscribeVisibility((hidden) => seen.push(hidden));
    expect(seen).toEqual([true]);
    release();
  });

  it('listens only while something is subscribed', () => {
    const release = subscribeVisibility(() => {});
    expect(handlers.get('visibilitychange')?.size).toBe(1);
    release();
    expect(handlers.get('visibilitychange')?.size).toBe(0);
  });

  it('schedules nothing of its own', async () => {
    vi.useFakeTimers();
    const release = subscribeVisibility(() => {});
    state = 'hidden';
    fire();
    expect(vi.getTimerCount()).toBe(0);
    release();
    vi.useRealTimers();
  });
});
