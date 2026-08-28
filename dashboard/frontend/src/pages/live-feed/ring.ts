import { onNextFrame, type Cancel } from '../../lifecycle/timers';

/**
 * The Live Feed's bounded buffer, and the frame coalescing over it.
 *
 * **Bounded, and bounded by construction.** The backing array is allocated once
 * at its capacity and never grows: the ring overwrites its oldest slot, so the
 * feed's memory is fixed whatever the query rate or the uptime is. There is no
 * server-side query store behind this — FastAdHunter keeps no per-query record
 * — so what falls out of the ring is gone, and the page says so.
 *
 * **Rendering is coalesced onto an animation frame.** A household at a few
 * hundred queries a second would otherwise re-render per event; instead a burst
 * costs one render per frame. It also gives the hidden-page behaviour for free:
 * a hidden document fires no frames, so rendering stops without a visibility
 * handler of the feed's own, while the ring keeps absorbing until the socket's
 * existing 30 s grace tears it down.
 */

/** Desktop capacity, and the narrow-viewport one. Fixed by the task; the
 *  artboards print whichever is in force on the page itself. */
export const DESKTOP_CAPACITY = 500;
export const NARROW_CAPACITY = 200;

/** The breakpoint the phone layout uses, so the ring and the layout switch
 *  together. */
export const NARROW_QUERY = '(max-width: 767px)';

/**
 * Sampled **once per page mount** (X6): the application has no viewport
 * listener anywhere, and adding the first one to resize a ring would be a
 * listener that outlives nothing useful. A rotation mid-visit keeps the
 * mount-time capacity until the next entry, and the on-page line prints
 * whichever is in force.
 */
export function ringCapacity(matches: (query: string) => boolean): number {
  return matches(NARROW_QUERY) ? NARROW_CAPACITY : DESKTOP_CAPACITY;
}

/** The breakpoint as it stands now. `false` where `matchMedia` is absent, which
 *  is the desktop answer and the one the tests' bare jsdom needs. */
export function matchesNarrow(): boolean {
  return (
    typeof window.matchMedia === 'function' &&
    window.matchMedia(NARROW_QUERY).matches
  );
}

/**
 * The application's **one** viewport listener, and it exists because the feed
 * builds a single tree.
 *
 * The ring's capacity stays sampled once (X6): it is a memory bound, and
 * resizing a window is not a reason to reallocate one. The *layout* cannot be
 * sampled once, because `display: none` is what hides the tree that does not
 * belong — build only the desktop table and drag the window under 768 px and
 * the CSS hides the one tree that exists, leaving the pager counting rows over
 * an empty body. So the bound is fixed at mount and the layout follows the
 * viewport, which is the split the two actually have.
 *
 * A `MediaQueryList` without `addEventListener` — the shape the tests stub —
 * subscribes to nothing and reports nothing, leaving the mount-time answer
 * standing.
 */
export function observeNarrow(listener: (narrow: boolean) => void): () => void {
  if (typeof window.matchMedia !== 'function') return () => undefined;
  const list = window.matchMedia(NARROW_QUERY);
  if (typeof list.addEventListener !== 'function') return () => undefined;
  const handle = () => listener(list.matches);
  list.addEventListener('change', handle);
  return () => list.removeEventListener('change', handle);
}

export class BoundedRing<T> {
  private readonly slots: Array<T | undefined>;
  private next = 0;
  private filled = 0;
  private pushed = 0;

  constructor(readonly capacity: number) {
    this.slots = new Array<T | undefined>(capacity);
  }

  push(item: T): void {
    this.slots[this.next] = item;
    this.next = (this.next + 1) % this.capacity;
    this.pushed += 1;
    if (this.filled < this.capacity) this.filled += 1;
  }

  /**
   * The sequence number of the oldest held item — total pushed minus what is
   * still held.
   *
   * **A row's identity, which its position is not.** The rendered list is
   * reversed and paged, so a key built from the index changed meaning between
   * flushes as new events arrived: the same DOM node was reused for a different
   * event, and a node built for one was discarded. Counting from the start of
   * the process gives every event a number nothing later can shift.
   */
  get firstSequence(): number {
    return this.pushed - this.filled;
  }

  get length(): number {
    return this.filled;
  }

  /** Oldest first. A fresh array per call, so a consumer holding the previous
   *  one sees a stable snapshot — which is what `pause` freezes. */
  items(): T[] {
    const out: T[] = [];
    const start = this.filled < this.capacity ? 0 : this.next;
    for (let step = 0; step < this.filled; step += 1) {
      const item = this.slots[(start + step) % this.capacity];
      if (item !== undefined) out.push(item);
    }
    return out;
  }

  clear(): void {
    this.slots.fill(undefined);
    this.next = 0;
    this.filled = 0;
  }
}

/**
 * A ring plus the one pending frame that renders it.
 *
 * `schedule` is the seam: the page takes the default animation frame, and a
 * test injects a synchronous one rather than driving jsdom's frame clock.
 */
export class FeedBuffer<T> {
  private readonly ring: BoundedRing<T>;
  /**
   * Whether a flush is already owed. Kept separately from the cancel below
   * because a **synchronous** scheduler — which is what the tests inject —
   * runs its callback before `schedule()` returns: assigning the cancel as the
   * only "is one pending" signal left a stale non-null value behind after the
   * callback had already cleared it, and the buffer then never scheduled
   * again. A real animation frame hides that, which is exactly why the seam is
   * worth having.
   */
  private scheduled = false;
  private release: Cancel | null = null;

  constructor(
    capacity: number,
    /** `firstSequence` is the oldest held item's number, so the renderer can
     *  key a row by identity rather than by its shifting position. */
    private readonly onFlush: (items: T[], firstSequence: number) => void,
    private readonly schedule: (run: () => void) => Cancel = onNextFrame,
  ) {
    this.ring = new BoundedRing<T>(capacity);
  }

  get capacity(): number {
    return this.ring.capacity;
  }

  get length(): number {
    return this.ring.length;
  }

  get firstSequence(): number {
    return this.ring.firstSequence;
  }

  /** Absorbs the event immediately; the render waits for the frame. */
  push(item: T): void {
    this.ring.push(item);
    if (this.scheduled) return;
    this.scheduled = true;
    const release = this.schedule(() => {
      this.scheduled = false;
      this.release = null;
      this.onFlush(this.ring.items(), this.ring.firstSequence);
    });
    // Still owed means the callback has not run yet, so the cancel is live.
    if (this.scheduled) this.release = release;
  }

  items(): T[] {
    return this.ring.items();
  }

  /** Empties ring and buffer, and renders the empty result at once — a Clear
   *  that waited for the next event would look broken on an idle feed. */
  clear(): void {
    this.ring.clear();
    this.cancel();
    this.onFlush([], this.ring.firstSequence);
  }

  /** Releases the pending frame. Called on unmount, so nothing renders into a
   *  page that is gone. */
  dispose(): void {
    this.cancel();
  }

  private cancel(): void {
    this.release?.();
    this.release = null;
    this.scheduled = false;
  }
}
