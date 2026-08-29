import { useEffect, useRef, useState } from 'preact/hooks';
import { refreshAllLists } from '../../api/lists';
import type { RefreshAllResponse } from '../../api/types';
import { ErrorState } from '../../components/error-state';
import { useFocusTrap } from '../../components/focus-trap';
import { StatusPill } from '../../components/status-pill';

/**
 * **Refresh all is synchronous and blocking, and is shown as what it is.**
 * `POST /lists/refresh` returns once the whole batch has been fetched and the
 * ruleset rebuilt once — so this states what is happening and waits. It does
 * **not** fake per-list progress it cannot know: the server sends one response
 * at the end, and a bar that advanced on a guess would be a lie about a
 * mutation.
 *
 * That is also what makes it different from the per-row Refresh, which is
 * `202 Accepted` and reports later through an event. The two must not look
 * alike.
 */
export function RefreshAllDialog({
  onClose,
  onDone,
}: {
  onClose: () => void;
  onDone: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const [result, setResult] = useState<RefreshAllResponse | null>(null);
  const [error, setError] = useState<Error | null>(null);

  useFocusTrap(true, () => dialog.current, onClose);

  useEffect(() => {
    const controller = new AbortController();
    refreshAllLists(controller.signal)
      .then((response) => {
        setResult(response);
        onDone();
      })
      .catch((cause: unknown) => {
        if (cause instanceof Error && cause.name === 'AbortError') return;
        setError(cause instanceof Error ? cause : new Error(String(cause)));
      });
    return () => controller.abort();
  }, [onDone]);

  const busy = result === null && error === null;

  return (
    <div class="dialog-scrim">
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Refresh all lists"
        ref={dialog}
      >
        <h2>Refreshing every enabled list</h2>

        {busy && (
          <>
            <p class="note" aria-live="polite">
              One pass over every enabled list, then a single recompile — not one
              per list. A list whose fetch fails is reported and skipped; the
              rest still refresh. This blocks until the whole batch is done.
            </p>
            <div class="progress-indeterminate" role="progressbar" aria-label="Refreshing" />
          </>
        )}

        {error !== null && <ErrorState error={error} />}

        {result !== null && (
          <>
            <p class="note">
              {/* R17 — `failed` counts every list that did not refresh,
                  rejected ones included, so the two add up to `results`. */}
              <strong>{result.refreshed.toLocaleString()}</strong> refreshed ·{' '}
              <strong>{result.failed.toLocaleString()}</strong> failed, rejected
              lists included — {result.results.length.toLocaleString()} in all.
            </p>
            <div class="refresh-results">
              {result.results.map((row) => (
                <div class="refresh-result" key={row.id}>
                  <span class="mono">{row.id}</span>
                  <StatusPill status={row.status} />
                  {row.error !== undefined && (
                    <span class="note refresh-result-error">{row.error}</span>
                  )}
                  {row.rules_active_dns !== undefined && (
                    <span class="note mono">
                      {row.rules_active_dns.toLocaleString()} dns rules
                    </span>
                  )}
                </div>
              ))}
            </div>
          </>
        )}

        <div class="dialog-actions">
          <button type="button" class="btn" onClick={onClose} disabled={busy}>
            {busy ? 'Working…' : 'Close'}
          </button>
        </div>
      </div>
    </div>
  );
}
