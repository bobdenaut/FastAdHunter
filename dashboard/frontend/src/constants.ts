/** Neither inventory changes fast — a client appears when it first resolves, a
 *  list on an operator action or on its own `refresh_hours` — so `clients` and
 *  `lists` take `telemetry`'s pair rather than a shorter option. */
export const REFRESH_OPTIONS_SECS = {
  health: [30, 60, 300],
  telemetry: [60, 300, 3600],
  cache: [60, 300, 3600],
  clients: [60, 300, 3600],
  lists: [60, 300, 3600],
} as const;

/** The hour is the default for the four the Dashboard reads: none of them is a
 *  liveness reading, and a page left open all day was issuing 288 requests an
 *  endpoint to redraw figures that move on the hour. `health` keeps its minute
 *  — it is what says the box is still answering, and an hour-old `live` badge
 *  would be a claim nobody made. */
export const REFRESH_DEFAULT_SECS = {
  health: 60,
  telemetry: 3600,
  cache: 3600,
  clients: 3600,
  lists: 3600,
} as const;

export const REFRESH_LABELS: Record<number, string> = {
  30: '30 s',
  60: '1 m',
  300: '5 m',
  3600: '1 h',
};

export const AGE_TICK_MS = 30_000;
export const HIDDEN_CLOSE_GRACE_MS = 30_000;
export const BACKOFF_MS = [1_000, 2_000, 4_000, 8_000, 15_000, 30_000];
export const BACKOFF_JITTER = 0.2;
export const PROBE_AFTER_FAILURES = 3;
export const PROBE_FAILURE_WINDOW_MS = 2_000;
export const PROBE_DIAGNOSTIC_CYCLES = 2;
export const OPEN_STABLE_MS = 1_000;
export const BUDGET_BYTES = 150 * 1024;

export const THEME_STORAGE_KEY = 'fah-theme';
export const DEV_GALLERY_MARKER = '__fah_dev_gallery__';
