import { useMemo, useState } from 'preact/hooks';
import type uPlot from 'uplot';
import { Card } from '../components/card';
import {
  barLabelsPlugin,
  createHoverState,
  hoverPlugin,
  stackedBarsOptions,
} from '../charts/stacked-bars';
import { readChartTheme } from '../charts/theme';
import { Chart } from '../components/chart';
import { ConfirmDialog } from '../components/confirm-dialog';
import { Donut } from '../components/donut';
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
import { queryTypeSlices } from '../derive';
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

const QUERY_TYPES = queryTypeSlices({
  A: 114224,
  AAAA: 44216,
  HTTPS: 14738,
  PTR: 7369,
  NS: 2400,
  SOA: 1286,
});

/**
 * A static 24-bucket series in the real aligned shape — `[xs, queries,
 * blocked]` — so the gallery exercises the option factory the Dashboard uses,
 * axes and both bar series included, without a live API. It is also where a
 * missing uPlot structural rule would show up, now that the vendor stylesheet
 * is gone.
 */
const HOURLY = [
  4000, 3200, 2600, 2400, 2200, 2500, 3700, 6100, 8200, 9200, 9500, 9700, 10000,
  9900, 9600, 10000, 10800, 11900, 13169, 12800, 11600, 9500, 6800, 5000,
];

const GALLERY_START = Date.UTC(2026, 7, 20, 10, 0, 0) / 1000;

const SERIES: uPlot.AlignedData = [
  HOURLY.map((_, index) => GALLERY_START + index * 3600),
  HOURLY,
  HOURLY.map((queries) => Math.round(queries * 0.14)),
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
  // Memoised on nothing here because the gallery's range and theme never
  // change within a render pass; the Dashboard keys the same factory on
  // `(range, theme)`, which is what finding m4 requires.
  const chartOptions = useMemo(() => {
    const theme = readChartTheme();
    const hover = createHoverState();
    return stackedBarsOptions({
      resolution: 'hour',
      theme,
      hover,
      plugins: [
        barLabelsPlugin(theme, hover),
        hoverPlugin({
          resolution: 'hour',
          hover,
          item: (index) => {
            const queries = HOURLY[index];
            if (queries === undefined) return null;
            const blocked = Math.round(queries * 0.14);
            return {
              ts: new Date((GALLERY_START + index * 3600) * 1000).toISOString(),
              queries,
              blocked,
              blockedPercent: 14,
            };
          },
        }),
      ],
    });
  }, []);

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
            <StatusPill status="never" /> <StatusPill status="disabled" />{' '}
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
          <Chart data={SERIES} options={chartOptions} decimatedBy={4} />
          <div class="chart-legend">
            <span>
              <span class="sw" style={{ background: 'var(--series-permitted)' }} />
              permitted
            </span>
            <span>
              <span class="sw" style={{ background: 'var(--series-blocked)' }} />
              blocked
            </span>
          </div>
        </Card>

        <Card title="Query types" bodyClass="donut-body">
          <Donut
            segments={QUERY_TYPES.map((slice, index) => ({
              label: slice.label,
              value: slice.value,
              colour: `var(--series-${String(index + 1)})`,
            }))}
            label="Query types by share"
          />
          <table class="donut-legend">
            <tbody>
              {QUERY_TYPES.map((slice) => (
                <tr key={slice.label}>
                  <td>{slice.label}</td>
                  <td class="num">{slice.value.toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </div>

      <div class="row c2">
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
