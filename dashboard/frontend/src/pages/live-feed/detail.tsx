import type { QueryEvent } from '../../api/types';
import { formatBytes } from '../../charts/format';
import { VerdictPill, type Verdict } from '../../components/verdict-pill';

/**
 * D20 — the Detail cell, from documented fields only.
 *
 * DNS: the record type and `endpoint N` when the key is present — which is
 * only on an item an upstream answered. HTTP and intercepted HTTPS: method,
 * resource type, status and relayed bytes.
 *
 * `https-sni` is neither. The SNI leg decides before a request exists, so
 * `ports.rs::as_http` hands it a `RequestEvent` whose method and path are
 * empty — placeholders, not observations. Its status is 0 on a judged hello
 * and a synthesized 408 or 400 (API.md §Events) on a connection closed before
 * one, so the status is drawn only when non-zero, beside the relayed byte
 * count. The empty-part filter covers the same placeholders on an `https`
 * session row, which the failure paths emit with no method and no path.
 *
 * DNS is the arm tested for, not HTTP: three of the four kinds carry the
 * request shape, and branching on `kind === 'http'` sent both Phase 3 kinds
 * through the DNS branch to render an empty record type.
 *
 * The cache outcome lives in its own cell ([`FeedCache`]), not here.
 */
export function Detail({ row }: { row: QueryEvent }) {
  if (row.kind === 'dns') {
    return (
      <span class="feed-detail">
        {row.qtype ?? '—'}
        {row.endpoint !== undefined && (
          <span class="feed-marker">endpoint {row.endpoint}</span>
        )}
      </span>
    );
  }
  if (row.kind === 'https-sni') {
    return (
      <span class="feed-detail">
        {[
          row.status === null || row.status === 0 ? null : String(row.status),
          row.bytes === null ? '—' : formatBytes(row.bytes),
        ]
          .filter((part): part is string => part !== null)
          .join(' · ')}
      </span>
    );
  }
  return (
    <span class="feed-detail">
      {[
        row.method,
        row.resource_type,
        row.status === null || row.status === 0 ? null : String(row.status),
        row.bytes === null ? null : formatBytes(row.bytes),
      ]
        .filter((part): part is string => part !== null && part !== '')
        .join(' · ')}
    </span>
  );
}

/**
 * The cache outcome for one row — `HIT`, `MISS`, or `—` where the question was
 * never asked of the cache.
 *
 * Two rows carry no outcome rather than a false `MISS`: anything that is not a
 * DNS query, which has no cache to consult — HTTP, the SNI leg and an
 * intercepted HTTPS request alike — and a blocked DNS query, which never
 * reaches the cache at all (ADR-0001 — which is also why
 * `cache_hits + cache_misses` counts `pass + allow` and never `block`).
 * Drawing either as a miss would invent a lookup that did not happen, which is
 * what testing `kind === 'http'` did to both Phase 3 kinds: the wire sets
 * `cached: false` on them because they never asked, and the cell read that as
 * a miss.
 *
 * **A hit is not a verdict.** A cache hit is still a `pass`; the verdict cell
 * says what was decided and this one says where the answer came from.
 */
export function FeedCache({ row }: { row: QueryEvent }) {
  if (row.kind !== 'dns' || row.verdict === 'block') {
    return <span class="note">—</span>;
  }
  return (
    <span class={`mono feed-cache feed-cache-${row.cached ? 'hit' : 'miss'}`}>
      {row.cached ? 'HIT' : 'MISS'}
    </span>
  );
}

const KNOWN: readonly string[] = ['pass', 'allow', 'block'];

/**
 * The verdict vocabulary is **closed**. `ports.rs::verdict_str` answers
 * `pass`, `allow` or `block` and nothing else; a frame carrying anything else
 * renders its literal text and joins no pill tone, because inventing a colour
 * for an unknown verdict is inventing a meaning for it.
 */
export function FeedVerdict({ verdict }: { verdict: string }) {
  if (KNOWN.includes(verdict)) {
    return <VerdictPill verdict={verdict as Verdict} />;
  }
  return <span class="mono feed-unknown-verdict">{verdict}</span>;
}
