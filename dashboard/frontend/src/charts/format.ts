import type { HistoryResolution } from '../api/types';

/**
 * The chart's number and time vocabulary. Pure functions with no uPlot import,
 * so a page may read them without pulling the chart chunk.
 */

function oneDecimal(value: number): string {
  const text = value.toFixed(1);
  return text.endsWith('.0') ? text.slice(0, -2) : text;
}

/**
 * `13.2k`, `4k`, `0` — the y-axis and bar-label form the artboards draw. A
 * trailing `.0` is dropped, so 4,021 reads `4k` rather than `4.0k`.
 */
export function compactCount(value: number): string {
  const abs = Math.abs(value);
  if (abs < 1000) return String(Math.round(value));
  if (abs < 1_000_000) return `${oneDecimal(value / 1000)}k`;
  if (abs < 1_000_000_000) return `${oneDecimal(value / 1_000_000)}M`;
  return `${oneDecimal(value / 1_000_000_000)}G`;
}

/** The bare figure, no sign: the artboards put a space before `%` in a card
 *  title bar and none on a tile, and that is the caller's choice. */
export function percent1(value: number): string {
  return value.toFixed(1);
}

const HOUR_MS = 3_600_000;

const CLOCK = new Intl.DateTimeFormat(undefined, {
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
});

const UTC_DAY = new Intl.DateTimeFormat(undefined, {
  timeZone: 'UTC',
  day: 'numeric',
  month: 'short',
});

const LOCAL_DAY = new Intl.DateTimeFormat(undefined, {
  day: 'numeric',
  month: 'short',
});

/**
 * The tooltip's first line. An hourly bucket is a local-clock window; a daily
 * one is a **UTC** day, because that is the boundary the rollups are summed
 * over — printing it in local time would name a day the figure does not cover.
 */
export function bucketWindowLabel(
  ts: string,
  resolution: HistoryResolution,
): string {
  const start = new Date(ts);
  if (Number.isNaN(start.getTime())) return ts;
  if (resolution === 'day') return `${UTC_DAY.format(start)} · UTC day`;
  return `${CLOCK.format(start)} – ${CLOCK.format(new Date(start.getTime() + HOUR_MS))}`;
}

/** The x-axis tick. uPlot is given epoch seconds, so this takes them too. */
export function axisTimeLabel(
  epochSeconds: number,
  resolution: HistoryResolution,
): string {
  const at = new Date(epochSeconds * 1000);
  return resolution === 'day' ? LOCAL_DAY.format(at) : CLOCK.format(at);
}

const MIB = 1024 * 1024;

/**
 * `1.1 MiB`, `64 MiB`, `2.7 MiB` — the artboards' byte form, and a display-unit
 * conversion only. The figure it renders is `bytes` against `max_bytes`, which
 * API.md calls a coarse per-entry estimate excluding the hash-table slabs; the
 * unit does not make it an allocator audit.
 */
export function formatMiB(bytes: number): string {
  return `${oneDecimal(bytes / MIB)} MiB`;
}

/** A latency tile's figure: `0.039`, `0.412`. Three decimals because the
 *  in-engine stages sit in the tens of microseconds and two would round two of
 *  the three to the same number. */
export function latencyMsLabel(ms: number): string {
  return ms.toFixed(3);
}

/**
 * `1.84` from `last_duration_micros`, `4.7` from a clean's `duration_ms` — the
 * artboard's two sweep figures, from one rule: two decimals, second one dropped
 * when it is a zero. These are milliseconds with a fraction worth keeping and
 * no microsecond detail worth printing.
 */
export function millisLabel(ms: number): string {
  const text = ms.toFixed(2);
  return text.endsWith('0') ? text.slice(0, -1) : text;
}

/** `1.84` from `cache_cleanup.last_duration_micros` — the unit conversion and
 *  the formatting together, so neither happens in a card. */
export function microsLabel(micros: number): string {
  return millisLabel(micros / 1000);
}

/** The latency axis: `1.0`, `0.75`, `0.50`, `0.25`, exactly as the artboard
 *  draws them. */
export function msAxisLabel(ms: number): string {
  return ms >= 1 ? ms.toFixed(1) : ms.toFixed(2);
}

/** An RFC 3339 stamp as the epoch **seconds** uPlot's time scale takes. One
 *  helper rather than one `Date.parse(...) / 1000` per chart. */
export function epochSeconds(ts: string): number {
  return Date.parse(ts) / 1000;
}

/**
 * `12.5`, `28.4` — the QPS stat row's two figures. One decimal because `qps` is
 * `queries_delta` over the sampling interval and arrives fractional; it is a
 * formatting of a served field and nothing more (KTD9).
 */
export function qpsLabel(qps: number): string {
  return qps.toFixed(1);
}

const KIB = 1024;

/**
 * `0 B`, `62.1 KiB`, `1.4 MiB` — the Live Feed's `bytes`, which is the response
 * body relayed downstream. A block reads `0 B`, which is the figure that shows
 * what filtering saved.
 *
 * **The units are binary because the divisor is.** `KB` and `MB` are decimal SI
 * and this scales by 1024, so the earlier labels were 2.4 % and 4.9 % off what
 * they claimed — on the one page-set whose header exists to teach that a budget
 * in MB and a reading in MiB are not the same number.
 */
export function formatBytes(bytes: number): string {
  if (bytes < KIB) return `${String(Math.round(bytes))} B`;
  if (bytes < KIB * KIB) return `${oneDecimal(bytes / KIB)} KiB`;
  return `${oneDecimal(bytes / (KIB * KIB))} MiB`;
}
