import { THEME_STORAGE_KEY } from '../constants';

export type Theme = 'light' | 'dark';

/**
 * Every token lives in CSS and the switch is one attribute on `<html>`, so
 * nothing built from markup needs telling. **A canvas does**: uPlot paints its
 * axes and bars with literal colours and cannot inherit a custom property, so
 * from p5-06 the chart reads the tokens at option-build time and rebuilds when
 * they change. That is what `subscribeTheme` exists for, and its only caller.
 */

export function isTheme(value: unknown): value is Theme {
  return value === 'light' || value === 'dark';
}

/**
 * The system preference, used when nothing is stored. `matchMedia` is absent in
 * a non-browser test environment, so its absence is a light default rather than
 * a throw.
 */
export function systemTheme(): Theme {
  if (typeof matchMedia !== 'function') return 'light';
  return matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

/**
 * A browser with site data blocked throws on read as well as on write, so both
 * are wrapped and the system preference is what is left.
 */
export function storedTheme(): Theme | null {
  try {
    const raw = localStorage.getItem(THEME_STORAGE_KEY);
    return isTheme(raw) ? raw : null;
  } catch {
    return null;
  }
}

export function currentTheme(): Theme {
  return storedTheme() ?? systemTheme();
}

type ThemeListener = (theme: Theme) => void;

const listeners = new Set<ThemeListener>();
let systemWatch: (() => void) | null = null;

function announce(theme: Theme): void {
  for (const listener of listeners) listener(theme);
}

function apply(theme: Theme): void {
  const root = document.documentElement;
  root.dataset['theme'] = theme;
  root.style.colorScheme = theme;
  announce(theme);
}

/**
 * Refcounted, and it holds a `matchMedia` listener rather than a timer: with
 * nothing stored the effective theme follows the system preference, so a
 * subscriber that only watched `setTheme` would miss the switch that costs it
 * an unreadable chart. The last unsubscribe removes the listener.
 */
export function subscribeTheme(listener: ThemeListener): () => void {
  listeners.add(listener);
  if (systemWatch === null && typeof matchMedia === 'function') {
    const query = matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => {
      if (storedTheme() === null) announce(systemTheme());
    };
    query.addEventListener('change', onChange);
    systemWatch = () => query.removeEventListener('change', onChange);
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && systemWatch !== null) {
      systemWatch();
      systemWatch = null;
    }
  };
}

export function setTheme(theme: Theme): void {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // The choice still applies to this document; it just will not survive a
    // reload. Refusing to switch would be the worse failure.
  }
  apply(theme);
}

export function toggleTheme(): Theme {
  const next: Theme = currentTheme() === 'dark' ? 'light' : 'dark';
  setTheme(next);
  return next;
}

/**
 * The pre-paint script in `index.html` has already stamped the attribute. This
 * re-applies it from the same source so a document that was rendered without
 * that script (a test, a stripped host page) still agrees with the store.
 */
export function initTheme(): Theme {
  const theme = currentTheme();
  apply(theme);
  return theme;
}
