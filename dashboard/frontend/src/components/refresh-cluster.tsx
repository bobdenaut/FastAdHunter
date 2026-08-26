import type { ComponentChildren } from 'preact';
import { useState } from 'preact/hooks';
import { REFRESH_LABELS } from '../constants';
import { intervalOptions } from '../refresh/preferences';
import type { RefreshRegistry } from '../refresh/registry';
import { usePreferredInterval, useRefresh } from '../refresh/use-refresh';
import type { RefreshEndpoint } from '../router/routes';
import { Icon } from '../shell/icon';
import { DataAge } from './data-age';

/**
 * One group bound to one endpoint: data age · interval selector · mini Refresh,
 * in that order. **One cluster per distinct polled endpoint on a page, never
 * one per card** — a second card reading an endpoint that already has one
 * carries nothing, and a zone fed by the `stats` push or by `/history/*` has no
 * timer to control and carries nothing either.
 *
 * `header` is the placement a route reading exactly one polled endpoint uses
 * (`Cache`); `card` is the title-bar placement (`Health`, `Main`). The only
 * difference the artboards draw is the word "updated" before the age.
 *
 * This is the single component that may call the refresh registry. Confining
 * request-triggering to one named component is what keeps route-scoped fetching
 * provable.
 */
export function RefreshCluster({
  registry,
  endpoint,
  placement = 'card',
  secondary,
}: {
  registry: RefreshRegistry;
  endpoint: RefreshEndpoint;
  placement?: 'header' | 'card';
  secondary?: ComponentChildren;
}) {
  const { fetchedAt } = useRefresh(registry, endpoint);
  const seconds = usePreferredInterval(endpoint);
  const [refreshing, setRefreshing] = useState(false);

  return (
    <span class={placement === 'header' ? 'ctl' : 'ctl in-card'}>
      {secondary !== undefined && <span class="note">{secondary}</span>}
      <DataAge fetchedAt={fetchedAt} prefix={placement === 'header'} />
      {/* A native <select>: keyboard, screen reader and the mobile picker for
          free, at close to zero bytes. The artboards draw a static span because
          a real control renders an OS picker their hand-drawn fidelity cannot
          show. */}
      <select
        class="sel"
        value={String(seconds)}
        aria-label={`Refresh interval for ${endpoint}`}
        onChange={(event) => {
          const next = Number((event.target as HTMLSelectElement).value);
          // Changing the interval issues no request — Refresh is the control
          // for that.
          registry.setRefreshInterval(endpoint, next);
        }}
      >
        {intervalOptions(endpoint).map((option) => (
          <option key={option} value={String(option)}>
            {REFRESH_LABELS[option] ?? `${option} s`}
          </option>
        ))}
      </select>
      <button
        type="button"
        class="mini"
        title="Refresh now"
        aria-label={`Refresh ${endpoint} now`}
        disabled={refreshing}
        onClick={() => {
          setRefreshing(true);
          void registry.invalidate(endpoint).finally(() => setRefreshing(false));
        }}
      >
        <Icon name="refresh" size={13} />
      </button>
      <span class="visually-hidden" aria-live="polite">
        {refreshing ? `Refreshing ${endpoint}` : ''}
      </span>
    </span>
  );
}
