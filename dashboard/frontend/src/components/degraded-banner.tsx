import { Icon } from '../shell/icon';
import type { UpstreamMode } from '../derive';

/**
 * E25 — `degraded` explained in place rather than alarmed, and the explanation
 * is different under each strategy. Amber, never red: neither reading is an
 * outage, and the box is still answering throughout.
 *
 * **One module, two pages.** Upstreams raised it in p5-08 and Diagnostics ·
 * Health shows the same reading, so it moved out of `pages/upstreams/` here
 * rather than being restated — p5-09's acceptance criterion "explained
 * identically wherever it appears: one string in the code, not three" made
 * structural. `pages/system-invariants.test.ts` asserts the strings exist once
 * in `src/`.
 *
 * It carries no controls. One refresh cluster per polled endpoint is the rule,
 * and `/health` already has one in each page's header — a second pair of
 * controls for the same endpoint would show two different ages for one reading.
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
  // C14 — the configuration could not be read, so the reading is given without
  // being attributed to a strategy this page cannot name.
  return (
    <>
      The strategy could not be read. Under <span class="mono">adaptive</span> it
      means no endpoint is currently <span class="mono">healthy</span>.
    </>
  );
}
