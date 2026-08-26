import { describe, expect, it, vi } from 'vitest';
import { SubscriptionRegistry, sameUnion } from './subscriptions';

describe('the subscription registry', () => {
  it('reports the positive-count keys, in the documented order', () => {
    const registry = new SubscriptionRegistry();
    registry.acquire(['config_changed', 'query']);
    expect(registry.union()).toEqual(['query', 'config_changed']);
  });

  it('refcounts, so a release drops a type only at zero', () => {
    const registry = new SubscriptionRegistry();
    const first = registry.acquire(['stats']);
    const second = registry.acquire(['stats']);
    first();
    expect(registry.union()).toEqual(['stats']);
    second();
    expect(registry.union()).toEqual([]);
  });

  it('ignores a second release from the same acquisition', () => {
    const registry = new SubscriptionRegistry();
    const release = registry.acquire(['stats']);
    registry.acquire(['stats']);
    release();
    release();
    expect(registry.union()).toEqual(['stats']);
  });

  it('never empties between two routes that both want stats', () => {
    const registry = new SubscriptionRegistry();
    const leaving = registry.acquire(['stats']);
    const seen: string[][] = [];
    registry.subscribe((union) => seen.push([...union]));

    // The shell's transition order: acquire the incoming route first is wrong,
    // release-then-acquire is what it does — and the refcount is what keeps the
    // union non-empty across it.
    const entering = registry.acquire(['stats']);
    leaving();

    expect(seen.every((union) => union.length === 1)).toBe(true);
    entering();
    expect(registry.union()).toEqual([]);
  });

  it('announces only when something was acquired or released', () => {
    const registry = new SubscriptionRegistry();
    const listener = vi.fn();
    registry.subscribe(listener);
    registry.acquire([])();
    expect(listener).not.toHaveBeenCalled();
    registry.acquire(['query']);
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('stops announcing once a listener releases', () => {
    const registry = new SubscriptionRegistry();
    const listener = vi.fn();
    registry.subscribe(listener)();
    registry.acquire(['query']);
    expect(listener).not.toHaveBeenCalled();
  });
});

describe('sameUnion', () => {
  it('compares members and order', () => {
    expect(sameUnion(['stats'], ['stats'])).toBe(true);
    expect(sameUnion(['stats'], [])).toBe(false);
    expect(sameUnion(['stats', 'query'], ['query', 'stats'])).toBe(false);
  });
});
