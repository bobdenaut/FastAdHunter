import { useState } from 'preact/hooks';
import { Card } from '../components/card';
import { Chart } from '../components/chart';
import { ConfirmDialog } from '../components/confirm-dialog';
import { EmptyState } from '../components/empty-state';
import { ErrorState } from '../components/error-state';
import { RefreshCluster } from '../components/refresh-cluster';
import { StageBar } from '../components/stage-bar';
import { StatusPill } from '../components/status-pill';
import { FrequencyBar, Table } from '../components/table';
import { Tile } from '../components/tile';
import { VerdictPill } from '../components/verdict-pill';
import { ApiError } from '../api/core';
import { DEV_GALLERY_MARKER } from '../constants';
import { refresh } from '../services';
import { currentTheme, setTheme } from '../theme/theme';
import type { PageProps } from '../router/routes';

interface DomainRow {
  domain: string;
  hits: number;
}

const TOP: DomainRow[] = [
  { domain: 'api.example.org', hits: 4021 },
  { domain: 'cdn.example.net', hits: 3140 },
  { domain: 'time.example.com', hits: 2884 },
];

// A static series: this task proves the wrapper, never a chart bound to real
// data.
const SERIES: [number[], number[]] = [
  [0, 1, 2, 3, 4, 5, 6, 7],
  [12, 19, 15, 22, 31, 27, 24, 30],
];

/**
 * Dev-only. It renders every vocabulary component with static props and
 * declares `stats`, `/telemetry` and `/cache`, so an `npm run dev` session
 * against a live API exercises the socket, the union re-send after a forced
 * disconnect, the REST probe and the refresh refcount in a browser rather than
 * only in vitest.
 *
 * Two cards read the same endpoint on purpose: that is what proves request
 * dedupe, selector sync and one-request-per-manual-Refresh.
 *
 * The marker below is what the postbuild grep looks for. A dev-only route that
 * survives tree-shaking is exactly what reaches production unnoticed, so its
 * absence is asserted rather than trusted.
 */
export function DevGallery(_props: PageProps) {
  const [dialog, setDialog] = useState(false);

  return (
    <div data-marker={DEV_GALLERY_MARKER}>
      <div class="row c4">
        <Tile
          label="Total queries"
          figure="184,233"
          accent="volume"
          glyph="dashboard"
          footer="6 active clients"
          href="/clients"
        />
        <Tile
          label="Queries blocked"
          figure="23,411"
          accent="blocked"
          glyph="policies"
          footer="watch the live feed"
          href="/diagnostics/live-feed"
        />
        <Tile
          label="Percentage blocked"
          figure="12.7%"
          accent="ratio"
          glyph="performance"
        />
        <Tile
          label="HTTP refused"
          figure="0"
          accent="zero"
          glyph="upstreams"
        />
      </div>

      <div class="row c2">
        <Card
          title="Cache state"
          secondary="entries 7,261 / 10,000"
          tools={<RefreshCluster registry={refresh} endpoint="cache" />}
        >
          <StageBar
            segments={[
              { label: 'fresh', value: 7026, colour: 'var(--accent)' },
              { label: 'stale', value: 52, colour: '#6f5fbe' },
              { label: 'expired', value: 183, colour: '#d6dde5' },
            ]}
          />
        </Card>

        {/* The second card on the same endpoint carries no cluster: one cluster
            per distinct polled endpoint on a page, never one per card. */}
        <Card title="Cache, read again" secondary="same endpoint, no cluster">
          <p class="note">
            This card shares the one `/cache` request the cluster above
            controls. Refreshing there updates here, from one request.
          </p>
        </Card>
      </div>

      <div class="row c2">
        <Card
          title="Engine"
          tools={
            <RefreshCluster
              registry={refresh}
              endpoint="telemetry"
              secondary="since boot"
            />
          }
        >
          <Table
            columns={[
              { key: 'domain', header: 'Domain', cell: (row) => <span class="mono">{row.domain}</span> },
              { key: 'hits', header: 'Hits', numeric: true, cell: (row) => row.hits.toLocaleString() },
              {
                key: 'frequency',
                header: 'Frequency',
                width: '34%',
                cell: (row) => <FrequencyBar share={row.hits / 4021} />,
              },
            ]}
            rows={TOP}
            rowKey={(row) => row.domain}
          />
        </Card>

        <Card title="Verdicts and statuses">
          <p>
            <VerdictPill verdict="pass" /> <VerdictPill verdict="allow" />{' '}
            <VerdictPill verdict="block" />
          </p>
          <p>
            <StatusPill status="ok" /> <StatusPill status="degraded" />{' '}
            <StatusPill status="failed" /> <StatusPill status="rejected" />{' '}
            <StatusPill status="penalized" /> <StatusPill status="probing" />
          </p>
          <p class="note">
            `permitted` is the derived `queries − blocked` band and is never a
            verdict, so it has no pill.
          </p>
        </Card>
      </div>

      <div class="row c2">
        <Card title="Queries over time" secondary="static series">
          <Chart
            data={SERIES}
            options={{
              scales: { x: { time: false } },
              series: [{}, { label: 'queries', stroke: '#1f9dbb' }],
            }}
            decimatedBy={4}
          />
        </Card>

        <Card title="Empty and error states">
          <EmptyState>An empty result is not an error.</EmptyState>
          <ErrorState
            error={
              new ApiError(422, 'validation_failed', 'line 14: invalid rule syntax', null)
            }
          />
        </Card>
      </div>

      <div class="row">
        <Card title="Controls">
          <button type="button" class="btn" onClick={() => setDialog(true)}>
            Open confirm dialog
          </button>{' '}
          <button
            type="button"
            class="btn g"
            onClick={() => setTheme(currentTheme() === 'dark' ? 'light' : 'dark')}
          >
            Toggle theme
          </button>
        </Card>
      </div>

      {dialog && (
        <ConfirmDialog
          title="Clear the cache?"
          confirmLabel="Clear"
          onConfirm={() => setDialog(false)}
          onCancel={() => setDialog(false)}
        >
          Nothing is actually cleared — this is the gallery.
        </ConfirmDialog>
      )}
    </div>
  );
}

export default DevGallery;
