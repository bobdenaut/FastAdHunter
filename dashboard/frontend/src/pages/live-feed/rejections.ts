import type { Client, InterceptionDocument, QueryEvent } from '../../api/types';

/**
 * The rejection view's whole logic, as pure functions over the rows the feed's
 * ring already holds. **No second buffer and no history**: what has fallen out
 * of the ring is gone, exactly as it is for the filters beside it.
 */

/**
 * `525` on an `https` row — the synthesized status `intercept.rs` emits when the
 * client answered our minted leaf with a rejecting TLS alert (API.md §Events,
 * ADR-0008 step 2).
 *
 * `UnknownCA` is deliberately **not** this: it stays `status 0`. Which alert a
 * client sends is a property of its TLS stack, not of the cause (measured
 * 2026-09-11, p3-06-n3-alert-ab.md), so a 525 row alone does not say whether
 * the host pins or the client never trusted the CA — `summarizeClients` below
 * carries the evidence that can.
 */
export const REJECTED_STATUS = 525;

export function isRejection(row: QueryEvent): boolean {
  return row.kind === 'https' && row.status === REJECTED_STATUS;
}

export interface RejectionGroup {
  client: string;
  clientName: string | null;
  host: string;
  count: number;
  /** The `ts` of the newest event in the group, verbatim. */
  last: string;
}

/**
 * Client and host, which is what a group *is*. Stable across flushes, unlike
 * the position of a row whose neighbours keep arriving — the view keys its
 * open dialog and its in-flight write on it, so it is defined once, here.
 */
export function keyOf(client: string, host: string): string {
  return `${client} ${host}`;
}

/**
 * One entry per client and host, newest first.
 *
 * A pinned application retries, so the raw stream carries the same host from
 * the same client many times a minute (ADR-0008 §The operator's path). The
 * grouping is what makes that one decision instead of hundreds of rows, and the
 * count is the evidence of how hard it is retrying.
 */
export function groupRejections(
  rows: readonly QueryEvent[],
): RejectionGroup[] {
  const groups = new Map<string, RejectionGroup>();
  for (const row of rows) {
    if (!isRejection(row)) continue;
    const key = keyOf(row.client, row.domain);
    const found = groups.get(key);
    if (found === undefined) {
      groups.set(key, {
        client: row.client,
        clientName: row.client_name,
        host: row.domain,
        count: 1,
        last: row.ts,
      });
      continue;
    }
    found.count += 1;
    // The ring is in arrival order, so a later row is the newer one — but the
    // name may only have been resolved on one of them, and the row that has it
    // is the one worth showing.
    found.last = row.ts;
    if (row.client_name !== null) found.clientName = row.client_name;
  }
  // `ts` is RFC 3339 with its fraction trimmed — `…:00Z`, `…:00.5Z` and
  // `…:00.25Z` all occur — so lexical order is not instant order inside a
  // second. Parsed once per group, never per row or per comparison.
  return [...groups.values()]
    .map((group) => ({ group, at: instantOf(group.last) }))
    .sort((left, right) => right.at - left.at)
    .map((entry) => entry.group);
}

/** `Date.parse`, with an unparseable `ts` sorting last rather than turning the
 *  comparator's result into `NaN`. */
function instantOf(ts: string): number {
  const at = Date.parse(ts);
  return Number.isNaN(at) ? 0 : at;
}

/**
 * Trim, drop a trailing root dot, lowercase — the client-side half of
 * `fah_rules::interception::normalize`. It exists so `isExcluded` answers the
 * same question the engine's matcher does; it is **not** validation, which is
 * the server's and is reported through `details`.
 */
export function normalizeHost(host: string): string {
  return host.trim().replace(/\.+$/, '').toLowerCase();
}

/**
 * The document's `exclude_domains`, normalized once. Built when the document
 * changes, not per group per flush: the view asks `isExcluded` for every group
 * on every frame, and re-normalizing the whole list each time would be
 * O(groups × entries) of string work for an answer that only changes with the
 * document.
 */
export function exclusionsOf(list: readonly string[]): ReadonlySet<string> {
  return new Set(list.map(normalizeHost));
}

/**
 * Whether an entry already covers `host` — the `ExclusionSet::contains` walk:
 * exact, or any parent label suffix, because an entry covers itself and every
 * subdomain.
 *
 * A `true` here is why the view shows "excluded" rather than a button that
 * would only ever earn a `duplicate` rejection or add a redundant entry.
 */
export function isExcluded(
  host: string,
  exclusions: ReadonlySet<string>,
): boolean {
  let candidate = normalizeHost(host);
  for (;;) {
    if (exclusions.has(candidate)) return true;
    const dot = candidate.indexOf('.');
    if (dot === -1) return false;
    candidate = candidate.slice(dot + 1);
  }
}

/**
 * The document plus exactly the observed host, appended verbatim.
 *
 * **Verbatim and un-deduplicated on purpose.** The server normalises and is the
 * only authority on what duplicates what; a client-side dedupe would either
 * silently swallow a write the operator asked for or re-implement the matcher's
 * rules a version behind it. `clients` is carried through untouched — this is a
 * whole-document `PUT` and the view has no business editing the other list.
 */
export function withExclusion(
  current: InterceptionDocument,
  host: string,
): InterceptionDocument {
  return {
    clients: [...current.clients],
    exclude_domains: [...current.exclude_domains, host],
  };
}


/**
 * The engine's per-client account of the terminate leg, from
 * `GET /api/v1/clients` (`intercepted`, API.md §Clients): sessions in which the
 * client sent a request — so it accepted the minted leaf at that moment — and
 * its `525` events, each with the time of the last one. Kept by fah-stats
 * since the stats started, so it outlives this page and the ring.
 *
 * Measured 2026-09-11 (p3-06-n3-alert-ab.md): the alert a client sends names
 * its TLS stack, not the cause — Chromium without the CA and Spotify with it
 * both say `certificate_unknown`. So a 525 row cannot tell a pinning
 * application from a client that never trusted the CA; this cross-connection
 * account is the evidence that can. It states what was observed, never
 * whether the CA is installed: a permissive verifier completes too.
 */
export interface ClientSummary {
  client: string;
  clientName: string | null;
  rejections: number;
  hosts: number;
  /** `null` when the engine has no record of this client, or none completed. */
  completed: { count: number; last: string } | null;
}

export function summarizeClients(
  groups: readonly RejectionGroup[],
  clients: readonly Client[],
): ClientSummary[] {
  const byIp = new Map(clients.map((client) => [client.ip, client] as const));
  const summaries = new Map<string, ClientSummary>();
  for (const group of groups) {
    const found = summaries.get(group.client);
    if (found === undefined) {
      const known = byIp.get(group.client);
      const record = known?.intercepted;
      const completed =
        record !== undefined && record.completed > 0 && record.last_completed !== null
          ? { count: record.completed, last: record.last_completed }
          : null;
      summaries.set(group.client, {
        client: group.client,
        clientName: group.clientName ?? known?.name ?? null,
        rejections: group.count,
        hosts: 1,
        completed,
      });
      continue;
    }
    found.rejections += group.count;
    found.hosts += 1;
    if (group.clientName !== null) found.clientName = group.clientName;
  }
  return [...summaries.values()].sort(
    (left, right) => right.rejections - left.rejections,
  );
}
