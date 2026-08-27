import { useEffect, useRef, useState } from 'preact/hooks';
import { cleanCache } from '../../api/cache';
import type { CacheCleanResponse, CacheUsage } from '../../api/types';
import { formatMiB, millisLabel } from '../../charts/format';
import { Card } from '../../components/card';
import { ErrorState } from '../../components/error-state';
import type { RefreshRegistry } from '../../refresh/registry';
import { clockLabel } from '../../time';
import { CounterTable } from './counter-table';

interface CleanResult {
  response: CacheCleanResponse;
  /** Client receive time. The API keeps no clean history, so this panel is
   *  session state and does not survive leaving the page (E8). */
  at: number;
}

/**
 * The one write on these three pages.
 *
 * **No confirmation dialog and no busy modal.** This is a cache maintenance
 * call — a few milliseconds — not a recompile, so the blocking treatment
 * `p5-07` gives `PUT /rules/user` does not apply. The deliberate choice the
 * task asks for is the checkbox: purging the stale window gives up the
 * serve-stale insurance an upstream outage is survived on, and it is off on
 * every mount.
 *
 * **The request is not aborted on unmount.** The write must land; what is lost
 * by leaving is the result panel, which is stated rather than worked around. A
 * response that arrives after the page has gone deliberately **skips** the
 * invalidate: `registry.invalidate` fetches unconditionally, so firing it then
 * would issue a `/cache` read no active route owns.
 */
export function CleanCard({
  cache,
  registry,
}: {
  cache: CacheUsage | null;
  registry: RefreshRegistry;
}) {
  const [stale, setStale] = useState(false);
  const [pending, setPending] = useState(false);
  const [result, setResult] = useState<CleanResult | null>(null);
  const [error, setError] = useState<Error | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  function run() {
    setPending(true);
    setError(null);
    cleanCache(stale)
      .then((response) => {
        if (!mounted.current) return;
        setResult({ response, at: Date.now() });
        setPending(false);
        // The stage bar and both bounds move now rather than at the next
        // interval. Skipped above when the page has already gone.
        void registry.invalidate('cache');
      })
      .catch((cause: unknown) => {
        if (!mounted.current) return;
        setError(cause instanceof Error ? cause : new Error(String(cause)));
        setPending(false);
      });
  }

  return (
    <Card
      title="Clean now"
      secondary="instead of waiting for capacity eviction"
      className="clean-card"
    >
      <label class="clean-choice">
        {/* The box keeps the artboard's 15 px drawing; the target around it is
            44 × 44, exactly as the list toggle's does. */}
        <span class="cb-target">
          <input
            type="checkbox"
            checked={stale}
            disabled={pending}
            onChange={(event) =>
              setStale((event.target as HTMLInputElement).checked)
            }
          />
        </span>
        <span>
          <span class="clean-choice-title">
            <b>Also purge the stale window</b>{' '}
            {cache !== null && (
              // E7 — both figures are read from the live snapshot; nothing here
              // predicts what the clean will actually find.
              <span class="note">
                ({cache.expired.toLocaleString()} expired{' '}
                {cache.expired === 1 ? 'entry' : 'entries'} right now — this
                would remove {cache.stale.toLocaleString()} stale{' '}
                {cache.stale === 1 ? 'one' : 'ones'})
              </span>
            )}
          </span>
          <span class="note">
            Off by default. Stale entries are the insurance an upstream outage
            is survived on; purging them is an explicit admin choice, not tidying
            up.
          </span>
        </span>
      </label>

      <div class="page-buttons">
        <button type="button" class="btn" disabled={pending} onClick={run}>
          {pending ? 'Removing…' : 'Remove expired'}
        </button>
      </div>

      {error !== null && <ErrorState error={error} />}

      {result !== null && (
        <div class="clean-result">
          <div class="note">Last clean, {clockLabel(result.at)}</div>
          <CounterTable
            rows={[
              {
                label: 'expired removed',
                value: result.response.removed_expired.toLocaleString(),
              },
              {
                label: 'stale removed',
                value: result.response.removed_stale.toLocaleString(),
              },
              {
                label: 'entries before → after',
                value: `${result.response.entries_before.toLocaleString()} → ${result.response.entries_after.toLocaleString()}`,
              },
              { label: 'freed', value: formatMiB(result.response.freed_bytes) },
              {
                label: 'took',
                value: `${millisLabel(result.response.duration_ms)} ms`,
              },
            ]}
          />
          <p class="note">
            RSS does not fall by the freed amount: a clean returns each entry’s
            own heap but never shrinks the table slab.
          </p>
        </div>
      )}
    </Card>
  );
}
