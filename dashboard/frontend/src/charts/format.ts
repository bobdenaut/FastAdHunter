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
