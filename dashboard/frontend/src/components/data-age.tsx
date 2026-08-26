import { useEffect, useState } from 'preact/hooks';
import { nowMs, subscribeAgeTick } from '../lifecycle/timers';

/**
 * At a five-minute interval an unlabelled figure is a lie, and on the Dashboard
 * a `/telemetry` card sits beside 2-second-fresh push tiles. The age is the
 * cluster's first element so a polled endpoint cannot ship a Refresh without
 * one.
 *
 * The ticking clock is a timer, and it is the shared 30 s one in
 * `lifecycle/timers.ts` — refcounted, alive only while at least one of these is
 * mounted.
 */
export function formatAge(fetchedAt: number | null, now: number): string {
  if (fetchedAt === null) return 'not read yet';
  const seconds = Math.max(0, Math.round((now - fetchedAt) / 1000));
  if (seconds < 60) return `${seconds} s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} m ago`;
  const hours = Math.floor(minutes / 60);
  return `${hours} h ago`;
}

export function DataAge({
  fetchedAt,
  prefix = false,
}: {
  fetchedAt: number | null;
  prefix?: boolean;
}) {
  const [now, setNow] = useState(nowMs);

  useEffect(() => subscribeAgeTick(() => setNow(nowMs())), []);
  useEffect(() => setNow(nowMs()), [fetchedAt]);

  const text = formatAge(fetchedAt, now);
  return <span class="age">{prefix && fetchedAt !== null ? `updated ${text}` : text}</span>;
}
