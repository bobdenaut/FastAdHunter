import type { QueryEvent } from '../../api/types';

/**
 * D17 — the feed's filters, applied **in the browser over the rows the ring
 * holds** and nowhere else. There is no server-side query store to search: what
 * has fallen out of the ring is gone, and a filter that implied otherwise would
 * be the promise this page exists not to make.
 */

export interface FeedFilters {
  /** `''` is "all". The vocabulary is closed — `pass`, `allow`, `block` — and
   *  is the whole of `ports.rs::verdict_str`. */
  verdict: string;
  /** `''` is "all"; otherwise `dns` or `http`. */
  kind: string;
  /** Matched against the client name **and** the address, because the row
   *  shows whichever exists and an operator types whichever they know. */
  client: string;
  domain: string;
}

export const EMPTY_FILTERS: FeedFilters = {
  verdict: '',
  kind: '',
  client: '',
  domain: '',
};

export const VERDICTS = ['pass', 'allow', 'block'] as const;
export const KINDS = ['dns', 'http'] as const;

export function isFiltered(filters: FeedFilters): boolean {
  return (
    filters.verdict !== '' ||
    filters.kind !== '' ||
    filters.client.trim() !== '' ||
    filters.domain.trim() !== ''
  );
}

function contains(haystack: string | null, needle: string): boolean {
  return haystack !== null && haystack.toLowerCase().includes(needle);
}

export function matchesFilters(row: QueryEvent, filters: FeedFilters): boolean {
  if (filters.verdict !== '' && row.verdict !== filters.verdict) return false;
  if (filters.kind !== '' && row.kind !== filters.kind) return false;
  const client = filters.client.trim().toLowerCase();
  if (
    client !== '' &&
    !contains(row.client_name, client) &&
    !contains(row.client, client)
  ) {
    return false;
  }
  const domain = filters.domain.trim().toLowerCase();
  if (domain !== '' && !contains(row.domain, domain)) return false;
  return true;
}

export function applyFilters<T>(
  rows: readonly T[],
  filters: FeedFilters,
  /** How to reach the event inside a row. The page carries a sequence number
   *  alongside it, so the filter must not assume the row *is* the event. */
  eventOf: (row: T) => QueryEvent = (row) => row as unknown as QueryEvent,
): T[] {
  if (!isFiltered(filters)) return [...rows];
  return rows.filter((row) => matchesFilters(eventOf(row), filters));
}
