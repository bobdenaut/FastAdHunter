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

/**
 * A refresh time as the day over the clock, which is how the Lists table stacks
 * them: two short lines in a narrow column rather than one wide line.
 *
 * `time` is `null` where there is no timestamp to print — the day line then
 * carries the whole answer and stands alone.
 */
export interface RefreshLabel {
  day: string;
  time: string | null;
}

/**
 * `12 Aug` over `04:00`. The date always, never `today` or `yesterday`: two
 * refresh columns side by side are read against each other, and a word in one
 * beside a date in the other cannot be compared at a glance.
 *
 * `null` is `never`, which the API uses for a list not yet refreshed **in this
 * process**: a boot from the cached copy is a load, not a refresh.
 */
export function lastRefreshLabel(ts: string | null, _now: number): RefreshLabel {
  if (ts === null) return { day: 'never', time: null };
  const at = new Date(ts);
  if (Number.isNaN(at.getTime())) return { day: ts, time: null };
  return { day: DAY.format(at), time: CLOCK.format(at) };
}

/**
 * When the scheduler comes back to a list, in the same forms
 * [`lastRefreshLabel`] prints so the two read as one pair.
 *
 * Derived, not served: the API carries `last_refresh` and `refresh_hours` and
 * no next-refresh field, so this is their sum. Two consequences the column has
 * to state rather than hide — `last_refresh` is `null` until the first
 * successful refresh **in this process**, so a list that has not refreshed
 * since boot has no schedule to show, and a due time already past means the
 * scheduler has not reached it yet rather than that it was missed.
 */
export function nextRefreshLabel(
  ts: string | null,
  refreshHours: number,
  now: number,
): RefreshLabel {
  if (ts === null) return { day: 'unscheduled', time: null };
  const at = new Date(ts).getTime();
  if (Number.isNaN(at)) return { day: ts, time: null };
  const due = new Date(at + refreshHours * 60 * 60 * 1000);
  if (due.getTime() <= now) return { day: 'due', time: null };
  return { day: DAY.format(due), time: CLOCK.format(due) };
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
