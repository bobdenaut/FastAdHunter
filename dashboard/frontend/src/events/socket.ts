import {
  HIDDEN_CLOSE_GRACE_MS,
  PROBE_AFTER_FAILURES,
  PROBE_DIAGNOSTIC_CYCLES,
} from '../constants';
import { after, nowMs, type Cancel } from '../lifecycle/timers';
import {
  backoffDelay,
  isImmediateFailure,
  shouldResetAttempts,
} from './backoff';
import { runProbe, sendsToLogin, type ProbeOutcome } from './probe';
import { sameUnion, type SubscriptionRegistry } from './subscriptions';
import {
  EVENTS_PATH,
  isEventType,
  type ConnectionState,
  type EventType,
  type IndicatorState,
  type ServerMessage,
} from './types';

export interface SocketLike {
  send(data: string): void;
  close(code?: number): void;
  onopen: (() => void) | null;
  onclose: (() => void) | null;
  onerror: (() => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
}

export interface SocketManagerOptions {
  subscriptions: SubscriptionRegistry;
  open: (url: string) => SocketLike;
  /** Only a real authentication failure calls this. */
  onAuthFailure: () => void;
  url?: string;
  probe?: () => Promise<ProbeOutcome>;
  random?: () => number;
  onDrop?: (reason: string, frame: unknown) => void;
}

export const UPGRADE_REFUSED_DETAIL = 'session valid, upgrade refused';
export const UNREACHABLE_DETAIL = 'server unreachable';

type MessageListener = (data: Record<string, unknown>) => void;
type StateListener = () => void;

export function defaultEventsUrl(): string {
  const scheme = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${scheme}//${window.location.host}${EVENTS_PATH}`;
}

/**
 * The only thing that opens or closes the connection. Visibility is an input to
 * it — `setSuspended` sets a flag the manager interprets — never a second
 * authority: with two actors able to open and close, a reconnect can race the
 * grace timer and either tear down a connection a route just asked for, or
 * leave one open that nothing wants.
 */
export class SocketManager {
  private readonly options: SocketManagerOptions;
  private readonly url: string;
  private readonly probe: () => Promise<ProbeOutcome>;
  private readonly random: () => number;
  private readonly listeners = new Map<EventType, Set<MessageListener>>();
  private readonly stateListeners = new Set<StateListener>();
  private readonly releaseUnion: () => void;

  private connection: SocketLike | null = null;
  private connectionState: ConnectionState = 'closed';
  private suspended = false;
  private attempt = 0;
  private immediateFailures = 0;
  private probeCycles = 0;
  private everOpened = false;
  private attemptStartedAt = 0;
  private openedAt = 0;
  private sentUnion: readonly EventType[] | null = null;
  private detailLine: string | null = null;
  private backoffCancel: Cancel | null = null;
  private graceCancel: Cancel | null = null;
  private disposed = false;

  constructor(options: SocketManagerOptions) {
    this.options = options;
    this.url = options.url ?? defaultEventsUrl();
    this.probe = options.probe ?? runProbe;
    this.random = options.random ?? Math.random;
    this.releaseUnion = options.subscriptions.subscribe(() => {
      this.onUnionChanged();
    });
  }

  state(): ConnectionState {
    return this.connectionState;
  }

  /**
   * Three states, and only three. A closed socket is the correct steady state
   * on nine of the thirteen screens, so `not-needed-here` must not read as a
   * fault; only `reconnecting` is a problem being reported.
   */
  indicator(): IndicatorState {
    if (this.connectionState === 'open') return 'live';
    if (this.connectionState === 'closed' && this.union().length === 0) {
      return 'not-needed-here';
    }
    return 'reconnecting';
  }

  /** Secondary text inside `reconnecting`. Never a fourth state. */
  detail(): string | null {
    return this.indicator() === 'reconnecting' ? this.detailLine : null;
  }

  on(type: EventType, listener: MessageListener): () => void {
    const set = this.listeners.get(type) ?? new Set<MessageListener>();
    set.add(listener);
    this.listeners.set(type, set);
    return () => {
      set.delete(listener);
    };
  }

  subscribeState(listener: StateListener): () => void {
    this.stateListeners.add(listener);
    return () => {
      this.stateListeners.delete(listener);
    };
  }

  setSuspended(suspended: boolean): void {
    if (this.suspended === suspended) return;
    this.suspended = suspended;
    if (suspended) {
      // Pause now, close later: `visibilitychange` fires on every app switch,
      // screen lock and notification pull, and closing on the edge costs a TLS
      // handshake per glance at a phone.
      this.cancelBackoff();
      if (this.connectionState === 'open') this.armGraceClose();
      return;
    }
    // Cancelling the pending close is not the same as having a connection: the
    // transport can die inside the grace window, which leaves `backoff` with no
    // timer armed (`scheduleBackoff` declines to schedule while suspended).
    // Returning here on the strength of an armed grace timer would strand the
    // manager in `reconnecting` for ever — the phone's normal case, since a
    // screen lock is what drops the Wi-Fi in the first place.
    this.cancelGrace();
    if (this.union().length === 0) return;
    if (
      this.connectionState === 'closed' ||
      this.connectionState === 'backoff'
    ) {
      this.connect();
    }
  }

  dispose(): void {
    this.disposed = true;
    this.releaseUnion();
    this.cancelBackoff();
    this.cancelGrace();
    this.teardown(1000);
    this.connectionState = 'closed';
    this.listeners.clear();
    this.stateListeners.clear();
  }

  private union(): readonly EventType[] {
    return this.options.subscriptions.union();
  }

  private onUnionChanged(): void {
    if (this.disposed) return;
    const union = this.union();

    if (union.length === 0) {
      // Never `{"subscribe":[]}`. The server accepts it and substitutes a Ping,
      // but a silent socket holds one of 64 slots until TCP gives up, and a
      // closed one costs a handshake against nine screens that never wanted it.
      this.cancelBackoff();
      this.cancelGrace();
      this.teardown(1000);
      this.attempt = 0;
      this.immediateFailures = 0;
      this.probeCycles = 0;
      this.detailLine = null;
      this.connectionState = 'closed';
      this.announce();
      return;
    }

    if (this.connectionState === 'open') {
      this.sendUnion(union);
      return;
    }
    if (this.connectionState === 'closed' && !this.suspended) this.connect();
  }

  private connect(): void {
    if (this.disposed || this.suspended) return;
    if (this.union().length === 0) {
      // Declining is not enough: called from the backoff timer, a bare return
      // leaves `backoff` standing with no timer armed, and `onUnionChanged`
      // reconnects only from `closed` — the manager would be stranded.
      this.settleClosed();
      return;
    }
    this.cancelBackoff();
    this.teardown(1000);

    this.everOpened = false;
    this.attemptStartedAt = nowMs();
    this.connectionState = 'connecting';
    this.announce();

    const socket = this.options.open(this.url);
    this.connection = socket;
    socket.onopen = () => {
      if (this.connection !== socket) return;
      this.everOpened = true;
      this.openedAt = nowMs();
      this.connectionState = 'open';
      const union = this.union();
      if (union.length === 0) {
        this.teardown(1000);
        this.connectionState = 'closed';
      } else {
        this.sendUnion(union);
        if (this.suspended) this.armGraceClose();
      }
      this.announce();
    };
    socket.onmessage = (event) => {
      if (this.connection === socket) this.dispatch(event.data);
    };
    socket.onerror = () => {
      if (this.connection === socket) this.handleClose(socket);
    };
    socket.onclose = () => {
      if (this.connection === socket) this.handleClose(socket);
    };
  }

  /** The first frame after every open, and after every reconnect. */
  private sendUnion(union: readonly EventType[]): void {
    if (this.connection === null) return;
    if (this.sentUnion !== null && sameUnion(this.sentUnion, union)) return;
    this.connection.send(JSON.stringify({ subscribe: [...union] }));
    this.sentUnion = union;
  }

  private handleClose(socket: SocketLike): void {
    if (this.connection !== socket) return;
    this.detachHandlers(socket);
    this.connection = null;
    this.sentUnion = null;

    if (this.everOpened && shouldResetAttempts(nowMs() - this.openedAt)) {
      this.attempt = 0;
      this.immediateFailures = 0;
      this.probeCycles = 0;
      this.detailLine = null;
    }

    if (this.disposed || this.union().length === 0) {
      this.connectionState = 'closed';
      this.announce();
      return;
    }

    if (isImmediateFailure(this.everOpened, nowMs() - this.attemptStartedAt)) {
      this.immediateFailures += 1;
    }

    if (this.immediateFailures >= PROBE_AFTER_FAILURES) {
      // Cooldown: the counter resets when a probe fires, so the next one needs
      // PROBE_AFTER_FAILURES fresh immediate failures. At the 30 s cap that is
      // at least 90 s apart.
      this.immediateFailures = 0;
      this.connectionState = 'probing';
      this.announce();
      void this.runProbeCycle();
      return;
    }

    this.scheduleBackoff();
  }

  private async runProbeCycle(): Promise<void> {
    const outcome = await this.probe();
    if (this.disposed) return;

    if (sendsToLogin(outcome)) {
      this.cancelBackoff();
      this.cancelGrace();
      this.teardown(1000);
      this.connectionState = 'closed';
      this.detailLine = null;
      this.announce();
      this.options.onAuthFailure();
      return;
    }

    if (outcome === 'session-valid') {
      this.probeCycles += 1;
      if (this.probeCycles >= PROBE_DIAGNOSTIC_CYCLES) {
        // The one failure the probe structurally cannot classify: a valid
        // session and a refused upgrade look identical from the browser.
        this.detailLine = UPGRADE_REFUSED_DETAIL;
      }
    } else if (outcome === 'unreachable') {
      this.detailLine = UNREACHABLE_DETAIL;
    }

    this.scheduleBackoff();
  }

  private scheduleBackoff(): void {
    if (this.union().length === 0) {
      // The probe outlives the route that wanted the socket: the union emptied
      // while it was in flight, `onUnionChanged` already tore down and reset,
      // and arming a retry for nobody would only re-enter `backoff` — a state
      // nothing leaves once its timer has fired against an empty union.
      this.settleClosed();
      return;
    }
    this.connectionState = 'backoff';
    this.announce();
    if (this.suspended) return;
    const delay = backoffDelay(this.attempt, this.random);
    this.attempt += 1;
    this.backoffCancel = after(delay, () => {
      this.backoffCancel = null;
      this.connect();
    });
  }

  /** Nobody wants the socket: the only state that may stand is `closed`, with
   *  no detail line left to make the indicator read `reconnecting`. */
  private settleClosed(): void {
    this.detailLine = null;
    if (this.connectionState === 'closed') return;
    this.connectionState = 'closed';
    this.announce();
  }

  private armGraceClose(): void {
    this.cancelGrace();
    this.graceCancel = after(HIDDEN_CLOSE_GRACE_MS, () => {
      this.graceCancel = null;
      this.teardown(1000);
      this.connectionState = 'closed';
      this.announce();
    });
  }

  private cancelGrace(): void {
    if (this.graceCancel !== null) {
      this.graceCancel();
      this.graceCancel = null;
    }
  }

  private cancelBackoff(): void {
    if (this.backoffCancel !== null) {
      this.backoffCancel();
      this.backoffCancel = null;
    }
  }

  private teardown(code: number): void {
    const socket = this.connection;
    if (socket === null) return;
    this.connection = null;
    this.sentUnion = null;
    this.detachHandlers(socket);
    socket.close(code);
  }

  private detachHandlers(socket: SocketLike): void {
    socket.onopen = null;
    socket.onclose = null;
    socket.onerror = null;
    socket.onmessage = null;
  }

  /**
   * The envelope, and nothing more: the frame parses as JSON, `type` is a known
   * string, `data` is an object. A runtime schema over `data` would be
   * machinery paid for on every message to re-check a contract the server owns.
   */
  private dispatch(raw: unknown): void {
    if (typeof raw !== 'string') return this.drop('not a text frame', raw);
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      return this.drop('not JSON', raw);
    }
    if (typeof parsed !== 'object' || parsed === null) {
      return this.drop('not an object', raw);
    }
    const message = parsed as Partial<ServerMessage>;
    if (!isEventType(message.type)) return this.drop('unknown type', raw);
    const data = message.data;
    if (typeof data !== 'object' || data === null || Array.isArray(data)) {
      return this.drop('data is not an object', raw);
    }
    for (const listener of this.listeners.get(message.type) ?? []) {
      listener(data);
    }
  }

  private drop(reason: string, frame: unknown): void {
    this.options.onDrop?.(reason, frame);
  }

  private announce(): void {
    for (const listener of this.stateListeners) listener();
  }
}
