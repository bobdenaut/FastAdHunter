import type { Upstream } from '../../api/types';
import { Link } from '../../router/link';
import { upstreamStateCounts, type UpstreamMode } from '../../derive';

/**
 * D3 — the endpoint states in one line, **under `adaptive` only**.
 *
 * Under `fallback` every row publishes `state: healthy` because no health state
 * exists to report (API.md), so the line becomes a count of endpoints with no
 * states attached rather than three zeros dressed as good news. `unknown` is
 * `GET /config` having failed and reads the same way.
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
          {mode === 'fallback'
            ? ' — the fallback strategy publishes no health state, so there is none to count'
            : ' — the strategy could not be read, so no state count is shown'}
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
