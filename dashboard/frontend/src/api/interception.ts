import { ApiError, request } from './core';
import type { InterceptionDocument } from './types';

export const INTERCEPTION_PATH = '/api/v1/interception';

export function getInterception(
  signal?: AbortSignal,
): Promise<InterceptionDocument> {
  return request<InterceptionDocument>(INTERCEPTION_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}

/**
 * Replaces the whole document and returns what was stored.
 *
 * **Never aborted from the UI.** A `PUT` applies on the next accepted
 * connection with no restart, and the commit runs to completion server-side
 * whatever this browser does; cancelling would only lose the answer that says
 * which document is now live.
 *
 * A rejection changes nothing — neither the file nor the running scope — so
 * every caller may keep showing the document it already had.
 */
export function putInterception(
  document: InterceptionDocument,
): Promise<InterceptionDocument> {
  return request<InterceptionDocument>(INTERCEPTION_PATH, {
    method: 'PUT',
    body: document,
  });
}

export type DocumentList = 'clients' | 'exclude_domains';

/**
 * The `422` envelope's `details`, whose `reason` is closed (API.md
 * §Interception). `index` and `duplicate_of` are 0-based positions in the list
 * **as sent**, which is what lets an editor put the message on the line the
 * operator typed.
 */
export type DocumentErrorDetails =
  | { reason: 'shape' }
  | { reason: 'over_cap'; list: DocumentList; len: number; cap: number }
  | { reason: 'invalid_entry'; list: DocumentList; index: number; entry: string }
  | {
      reason: 'duplicate';
      list: DocumentList;
      index: number;
      entry: string;
      duplicate_of: number;
    };

function isList(value: unknown): value is DocumentList {
  return value === 'clients' || value === 'exclude_domains';
}

function index(record: Record<string, unknown>, key: string): boolean {
  const value = record[key];
  return typeof value === 'number' && Number.isInteger(value) && value >= 0;
}

/**
 * The structured rejection, or `null` when the failure is not one.
 *
 * **A runtime shape check, and the only thing either surface branches on.**
 * `message` is written for a human and is rendered verbatim where it is shown;
 * parsing it would build the UI on prose the contract does not fix. A `422`
 * without a recognised `details`, any other status, and anything that is not an
 * `ApiError` all read `null`, so a caller falls back to showing `message`.
 */
export function documentErrorDetails(
  error: unknown,
): DocumentErrorDetails | null {
  if (!(error instanceof ApiError) || error.status !== 422) return null;
  const details = error.details;
  if (typeof details !== 'object' || details === null) return null;
  const record = details as Record<string, unknown>;
  switch (record['reason']) {
    case 'shape':
      return { reason: 'shape' };
    case 'over_cap':
      return isList(record['list']) &&
        index(record, 'len') &&
        index(record, 'cap')
        ? {
            reason: 'over_cap',
            list: record['list'],
            len: record['len'] as number,
            cap: record['cap'] as number,
          }
        : null;
    case 'invalid_entry':
      return isList(record['list']) &&
        index(record, 'index') &&
        typeof record['entry'] === 'string'
        ? {
            reason: 'invalid_entry',
            list: record['list'],
            index: record['index'] as number,
            entry: record['entry'],
          }
        : null;
    case 'duplicate':
      return isList(record['list']) &&
        index(record, 'index') &&
        index(record, 'duplicate_of') &&
        typeof record['entry'] === 'string'
        ? {
            reason: 'duplicate',
            list: record['list'],
            index: record['index'] as number,
            entry: record['entry'],
            duplicate_of: record['duplicate_of'] as number,
          }
        : null;
    default:
      return null;
  }
}
