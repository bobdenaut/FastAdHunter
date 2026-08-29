import type { Stats, Telemetry } from '../../api/types';
import { Tile } from '../../components/tile';
import { percent1 } from '../../charts/format';
import { formatUptime } from '../../time';

/**
 * Tile row 1 — the rolling 24 h DNS window, straight from `/stats` and moved by
 * the socket's `stats` push every ~2 s. Every figure is a field: nothing on
 * this row is derived.
 *
 * `clientCount` is `/clients`' `items.length`, which is what the artboard's
 * "6 active clients" footer states and what `/stats` does not carry.
 */
export function DnsTiles({
  stats,
  clientCount,
}: {
  stats: Stats | null;
  clientCount: number | null;
}) {
  const clients =
    clientCount === null ? '—' : `${clientCount.toLocaleString()} active clients`;

  return (
    <div class="row c4 z-tiles-dns">
      <Tile
        label="Total queries"
        figure={figure(stats?.queries_total)}
        accent="volume"
        glyph="globe"
        footer={clients}
        footerShort={clientCount === null ? '—' : `${String(clientCount)} clients`}
        href="/clients"
      />
      {/* The three shortened labels are `MobileDashboard.dc.html`'s own:
          `Blocked`, `Blocked`, `Cache hit`. The two "Blocked" tiles are told
          apart by their figure and their footer, as the artboard draws them. */}
      <Tile
        label="Queries blocked"
        labelShort="Blocked"
        figure={figure(stats?.blocked_total)}
        accent="blocked"
        glyph="hand"
        footer="watch the live feed"
        footerShort="live feed"
        href="/diagnostics/live-feed"
      />
      <Tile
        label="Percentage blocked"
        labelShort="Blocked"
        figure={ratio(stats?.blocked_percent)}
        accent="ratio"
        glyph="pie"
        footer="of all DNS queries"
        footerShort="of DNS"
      />
      <Tile
        label="Cache hit rate"
        labelShort="Cache hit"
        figure={ratio(stats?.cache_hit_percent)}
        accent="ratio"
        glyph="cache"
        footer="inspect the cache"
        footerShort="cache"
        href="/cache"
      />
    </div>
  );
}

function figure(value: number | undefined): string {
  return value === undefined ? '—' : value.toLocaleString();
}

function ratio(value: number | undefined): string {
  return value === undefined ? '—' : `${percent1(value)}%`;
}

/**
 * Tile row 2 — the HTTP pipeline and the engine, and **a different window from
 * the row above it**. `/stats` is a rolling 24 h view; `counters.http` is
 * process-lifetime cumulative and returns to zero on restart (API.md
 * §telemetry). Two visually identical rows meaning different windows is the
 * trap the caption below exists to close, and it is a rendered element rather
 * than a tooltip because a trap nobody hovers is not closed.
 *
 * There is no 24 h HTTP figure in the API and none is derived here.
 */
export function HttpTiles({
  telemetry,
  status,
  enabledLists,
}: {
  telemetry: Telemetry | null;
  status: string | null;
  enabledLists: number | null;
}) {
  const http = telemetry?.counters.http;
  const requests =
    http === undefined ? undefined : http.pass + http.allow + http.block;
  const refused = http?.refused;
  const lists =
    enabledLists === null ? '—' : `${String(enabledLists)} lists`;

  return (
    <>
      <div class="row c4 z-tiles-http">
        <Tile
          label="HTTP requests"
          figure={figure(requests)}
          accent="volume"
          glyph="proxy"
          footer="proxy pipeline"
          footerShort="proxy"
        />
        <Tile
          label="HTTP blocked"
          figure={figure(http?.block)}
          accent="blocked"
          glyph="shield"
          footer={
            refused === undefined
              ? '—'
              : `${refused.toLocaleString()} refused by egress policy`
          }
          footerShort={
            refused === undefined ? '—' : `${refused.toLocaleString()} refused`
          }
        />
        <Tile
          label="Compiled rules"
          figure={figure(telemetry?.ruleset.rules)}
          accent="healthy"
          glyph="lists"
          footer="manage lists"
          footerShort={lists}
          href="/lists"
        />
        <Tile
          label="Uptime"
          figure={
            telemetry === undefined || telemetry === null
              ? '—'
              : formatUptime(telemetry.process.uptime_seconds)
          }
          accent="healthy"
          glyph="clock"
          footer={status === null ? '—' : `status ${status}`}
          footerShort={status === null ? '—' : `status ${status}`}
        />
      </div>
      <p class="note tile-caption z-caption">
        HTTP figures are <strong>since restart</strong> — the proxy's counters
        are cumulative for the life of the process, where the DNS row above is a
        rolling 24 h window. The API carries no 24 h HTTP figure.
      </p>
    </>
  );
}
