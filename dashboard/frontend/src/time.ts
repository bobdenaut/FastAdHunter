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

/**
 * `2 s ago` / `4 m ago` / `3 h ago` / `9 d ago` — the age of a reading, and the
 * age of a client's last query. Both artboards print the same four forms, so
 * there is one formatter.
 *
 * The day form is reached only by `last_seen` on the Clients page, never by a
 * poll age: a device that stopped asking a week ago stays listed, and
 * `168 h ago` is a figure nobody reads as a week.
 */
export function formatAge(fetchedAt: number | null, now: number): string {
  if (fetchedAt === null) return 'not read yet';
  const seconds = Math.max(0, Math.round((now - fetchedAt) / 1000));
  if (seconds < 60) return `${String(seconds)} s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${String(minutes)} m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${String(hours)} h ago`;
  return `${String(Math.floor(hours / 24))} d ago`;
}

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

/**
 * `2 s ago` / `1 m ago` / `2 h ago` / `9 d ago` — what both Clients artboards
 * print beside a client. It is a formatting of `last_seen` against the
 * render's own `now`, computed once per render: there is **no ticker** on
 * these pages, so a label does not re-render itself and no timer outlives the
 * route.
 */
export function lastSeenLabel(ts: string, now: number): string {
  const at = new Date(ts).getTime();
  return Number.isNaN(at) ? ts : formatAge(at, now);
}

/** `03:14` — the clock the Cache page prints beside a clean result. It is the
 *  client's receive time, not an API field: the API keeps no clean history, so
 *  the panel is session state and says only when this browser saw it. */
export function clockLabel(at: number): string {
  return CLOCK.format(new Date(at));
}
