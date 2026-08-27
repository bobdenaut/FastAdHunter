/** The four documented server→client message types (API.md §Events). */
export const EVENT_TYPES = [
  'query',
  'stats',
  'config_changed',
  'list_refreshed',
] as const;

export type EventType = (typeof EVENT_TYPES)[number];

export function isEventType(value: unknown): value is EventType {
  return (
    typeof value === 'string' &&
    (EVENT_TYPES as readonly string[]).includes(value)
  );
}

/**
 * The envelope, and only the envelope. `data` is handed to subscribers as the
 * server sent it: the types describe the contract and unknown fields are
 * ignored, so re-checking a documented shape on every message would be
 * machinery paid for per frame to police something the server owns.
 */
export interface ServerMessage {
  type: EventType;
  data: Record<string, unknown>;
}

export type ConnectionState =
  | 'closed'
  | 'connecting'
  | 'open'
  | 'backoff'
  | 'probing';

/** Three states, and only three, from the socket's point of view. A detail line
 *  is secondary text inside `reconnecting`, never a fourth state.
 *
 *  `api-unreachable` is not one of them: the socket never produces it. The
 *  shell substitutes it for `not-needed-here` when the API itself stopped
 *  answering, so a route that opens no socket still reports health rather than
 *  reporting the absence of a subscription. */
export type IndicatorState =
  | 'live'
  | 'not-needed-here'
  | 'reconnecting'
  | 'api-unreachable';

export const EVENTS_PATH = '/api/v1/events';
