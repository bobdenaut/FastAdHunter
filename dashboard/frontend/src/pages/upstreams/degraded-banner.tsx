import { Icon } from '../../shell/icon';
import type { UpstreamMode } from '../../derive';

/**
 * E25 — `degraded` explained in place rather than alarmed, and the explanation
 * is different under each strategy. Amber, never red: neither reading is an
 * outage, and the box is still answering throughout.
 *
 * It carries no controls. One refresh cluster per polled endpoint is the rule,
 * and `/health` already has one in the page header — a second pair of controls
 * for the same endpoint would show two different ages for one reading.
 */
export function DegradedBanner({ mode }: { mode: UpstreamMode }) {
  return (
    <div class="banner warn" role="status">
      <Icon name="warning" size={16} className="warning" />
      <div>
        <b>
          Health reports <span class="mono">degraded</span> — this is not an
          outage.
        </b>{' '}
        {explanation(mode)} Clients are being served.
      </div>
    </div>
  );
}

function explanation(mode: UpstreamMode): preact.ComponentChildren {
  if (mode === 'adaptive') {
    return (
      <>
        Under the <span class="mono">adaptive</span> strategy it means no
        endpoint is currently <span class="mono">healthy</span>: every one is
        penalized or being probed. Cache hits and serve-stale keep answering
        throughout, and a penalized endpoint is still queried when nothing better
        is left.
      </>
    );
  }
  if (mode === 'fallback') {
    return (
      <>
        Under the <span class="mono">fallback</span> strategy it means every
        endpoint carries a non-zero consecutive-failure count. A secondary is
        only attempted when the primary fails, so a streak there can be hours
        old.
      </>
    );
  }
  // C14 — the configuration could not be read, so both readings are given and
  // each is attributed to the strategy it belongs to. Guessing one would state
  // the opposite of the truth half the time.
  return (
    <>
      The strategy could not be read, so both readings apply: under{' '}
      <span class="mono">adaptive</span> it means no endpoint is currently{' '}
      <span class="mono">healthy</span>, and under{' '}
      <span class="mono">fallback</span> that every endpoint carries a non-zero
      consecutive-failure count.
    </>
  );
}
