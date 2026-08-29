/** Neither inventory changes fast — a client appears when it first resolves, a
 *  list on an operator action or on its own `refresh_hours` — so `clients` and
 *  `lists` take `telemetry`'s pair rather than a shorter option. */
export const REFRESH_OPTIONS_SECS = {
  health: [30, 60, 300],
  telemetry: [60, 300],
  cache: [60, 300],
  clients: [60, 300],
  lists: [60, 300],
} as const;

export const REFRESH_DEFAULT_SECS = {
  health: 60,
  telemetry: 300,
  cache: 300,
  clients: 300,
  lists: 300,
} as const;

export const REFRESH_LABELS: Record<number, string> = {
  30: '30 s',
  60: '1 m',
  300: '5 m',
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
