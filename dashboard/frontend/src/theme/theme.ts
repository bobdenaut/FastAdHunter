import { THEME_STORAGE_KEY } from '../constants';

export type Theme = 'light' | 'dark';

/**
 * There is no subscription: every token lives in CSS and the switch is one
 * attribute on `<html>`, so nothing re-renders and nothing needs telling.
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

function apply(theme: Theme): void {
  const root = document.documentElement;
  root.dataset['theme'] = theme;
  root.style.colorScheme = theme;
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
