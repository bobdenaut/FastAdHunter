/**
 * Durations and timestamps as the artboards print them. Formatting only — no
 * arithmetic over API figures happens here.
 */

const CLOCK = new Intl.DateTimeFormat(undefined, {
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
});

const DAY = new Intl.DateTimeFormat(undefined, {
  day: 'numeric',
  month: 'short',
});

const DAY_MS = 86_400_000;

/** `4h 31m`, as `Main.dc.html` draws the Uptime tile. */
export function formatUptime(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  if (total < 60) return `${total}s`;
  const minutes = Math.floor(total / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  if (days > 0) return `${days}d ${hours % 24}h`;
  if (hours > 0) return `${hours}h ${minutes % 60}m`;
  return `${minutes}m`;
}

function startOfLocalDay(at: Date): number {
  return new Date(at.getFullYear(), at.getMonth(), at.getDate()).getTime();
}

/**
 * `04:00` today, `yesterday 04:00` the day before, `12 Aug 04:00` further back
 * — the three forms `Lists.dc.html` draws. `null` is `never`, which the API
 * uses for a list not yet refreshed **in this process**: a boot from the cached
 * copy is a load, not a refresh.
 */
export function lastRefreshLabel(ts: string | null, now: number): string {
  if (ts === null) return 'never';
  const at = new Date(ts);
  if (Number.isNaN(at.getTime())) return ts;
  const today = startOfLocalDay(new Date(now));
  const day = startOfLocalDay(at);
  if (day === today) return CLOCK.format(at);
  if (day === today - DAY_MS) return `yesterday ${CLOCK.format(at)}`;
  return `${DAY.format(at)} ${CLOCK.format(at)}`;
}
