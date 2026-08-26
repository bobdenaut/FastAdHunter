import { EVENT_TYPES, type EventType } from './types';

type Listener = (union: readonly EventType[]) => void;

/**
 * A refcount per event type. The union is the keys whose count is above zero;
 * `subscribe` replaces on the wire, so the union is the whole outbound
 * protocol.
 *
 * The route transition acquires the incoming route's types **before** releasing
 * the outgoing route's (`lifecycle/route-lifecycle.ts`): two routes that both
 * want `stats` must not tear the socket down and rebuild it, and it is the
 * incoming acquisition landing first that keeps the count above zero across the
 * transition. Releasing first would empty the union for the length of one
 * statement, and the socket manager closes on the announcement.
 */
export class SubscriptionRegistry {
  private readonly counts = new Map<EventType, number>();
  private readonly listeners = new Set<Listener>();

  acquire(types: readonly EventType[]): () => void {
    for (const type of types) {
      this.counts.set(type, (this.counts.get(type) ?? 0) + 1);
    }
    if (types.length > 0) this.announce();

    let released = false;
    return () => {
      if (released) return;
      released = true;
      for (const type of types) {
        const next = (this.counts.get(type) ?? 0) - 1;
        if (next > 0) this.counts.set(type, next);
        else this.counts.delete(type);
      }
      if (types.length > 0) this.announce();
    };
  }

  /** Ordered by `EVENT_TYPES` so two unions with the same members compare
   *  equal, and a re-send after a reconnect is byte-identical. */
  union(): readonly EventType[] {
    return EVENT_TYPES.filter((type) => (this.counts.get(type) ?? 0) > 0);
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private announce(): void {
    const union = this.union();
    for (const listener of this.listeners) listener(union);
  }
}

export function sameUnion(
  a: readonly EventType[],
  b: readonly EventType[],
): boolean {
  return a.length === b.length && a.every((type, index) => type === b[index]);
}
