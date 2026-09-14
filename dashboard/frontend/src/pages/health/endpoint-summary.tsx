import type { Upstream } from '../../api/types';
import { Link } from '../../router/link';
import { upstreamStateCounts, type UpstreamMode } from '../../derive';

/**
 * D3 — the endpoint states in one line, **under `adaptive` only**.
 *
 * `unknown` is `GET /config` having failed, so the rule these states follow is
 * unknown too: the line becomes a count of endpoints with no states attached
 * rather than three figures dressed as a reading nobody vouched for.
 */
export function EndpointSummary({
  mode,
  upstreams,
}: {
  mode: UpstreamMode;
  upstreams: readonly Upstream[] | null;
}) {
  if (upstreams === null) {
    return <p class="note">Endpoints not read yet.</p>;
  }

  const total = upstreams.length;
  const plural = total === 1 ? 'endpoint' : 'endpoints';

  return (
    <p class="note health-endpoints">
      {mode === 'adaptive' ? (
        <>
          {total} {plural} — {summarise(upstreams)}
        </>
      ) : (
        <>
          {total} {plural}
          {' — the strategy could not be read, so no state count is shown'}
        </>
      )}{' '}
      <Link href="/upstreams" class="linky">
        Open Upstreams →
      </Link>
    </p>
  );
}

function summarise(upstreams: readonly Upstream[]): string {
  const counts = upstreamStateCounts(upstreams);
  return `${counts.healthy} healthy, ${counts.penalized} penalized, ${counts.probing} probing`;
}
