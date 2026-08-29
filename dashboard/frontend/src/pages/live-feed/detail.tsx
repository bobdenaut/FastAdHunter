import type { QueryEvent } from '../../api/types';
import { formatBytes } from '../../charts/format';
import { VerdictPill, type Verdict } from '../../components/verdict-pill';

/**
 * D20 — the Detail cell, from documented fields only.
 *
 * DNS: the record type, the `cached` marker when it was a hit, and
 * `endpoint N` when the key is present — which is only on an item an upstream
 * answered. HTTP: method, resource type, status and relayed bytes.
 *
 * **`cached` is a marker, not a verdict.** A cache hit is still a `pass`, and
 * the two are drawn differently for exactly that reason.
 */
export function Detail({ row }: { row: QueryEvent }) {
  if (row.kind === 'http') {
    return (
      <span class="feed-detail">
        {[
          row.method,
          row.resource_type,
          row.status === null ? null : String(row.status),
          row.bytes === null ? null : formatBytes(row.bytes),
        ]
          .filter((part): part is string => part !== null)
          .join(' · ')}
      </span>
    );
  }
  return (
    <span class="feed-detail">
      {row.qtype ?? '—'}
      {row.cached && <span class="feed-marker">cached</span>}
      {row.endpoint !== undefined && (
        <span class="feed-marker">endpoint {row.endpoint}</span>
      )}
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
