import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { THEME_STORAGE_KEY } from '../constants';
import {
  currentTheme,
  initTheme,
  isTheme,
  setTheme,
  storedTheme,
  systemTheme,
  toggleTheme,
} from './theme';

const store = new Map<string, string>();

function installStorage(throwing = false): void {
  vi.stubGlobal('localStorage', {
    getItem(key: string) {
      if (throwing) throw new Error('site data blocked');
      return store.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      if (throwing) throw new Error('site data blocked');
      store.set(key, value);
    },
  });
}

function installMatchMedia(dark: boolean): void {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: dark && query.includes('dark'),
  }));
}

beforeEach(() => {
  store.clear();
  installStorage();
  installMatchMedia(false);
  vi.stubGlobal('document', { documentElement: { dataset: {}, style: {} } });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('theme', () => {
  it('accepts only the two known values', () => {
    expect(isTheme('dark')).toBe(true);
    expect(isTheme('light')).toBe(true);
    expect(isTheme('sepia')).toBe(false);
    expect(isTheme(null)).toBe(false);
  });

  it('falls back to the system preference when nothing is stored', () => {
    installMatchMedia(true);
    expect(storedTheme()).toBeNull();
    expect(currentTheme()).toBe('dark');
  });

  it('ignores a stored value outside the two known ones', () => {
    store.set(THEME_STORAGE_KEY, 'sepia');
    expect(storedTheme()).toBeNull();
    expect(currentTheme()).toBe('light');
  });

  it('survives a throwing localStorage on read and on write', () => {
    installStorage(true);
    expect(storedTheme()).toBeNull();
    expect(() => setTheme('dark')).not.toThrow();
    expect(document.documentElement.dataset['theme']).toBe('dark');
  });

  it('stamps the root element and the colour scheme', () => {
    setTheme('dark');
    expect(document.documentElement.dataset['theme']).toBe('dark');
    expect(document.documentElement.style.colorScheme).toBe('dark');
    expect(store.get(THEME_STORAGE_KEY)).toBe('dark');
  });

  it('toggles between the two', () => {
    expect(toggleTheme()).toBe('dark');
    expect(toggleTheme()).toBe('light');
  });

  it('re-applies the stored choice on init', () => {
    store.set(THEME_STORAGE_KEY, 'dark');
    expect(initTheme()).toBe('dark');
    expect(document.documentElement.dataset['theme']).toBe('dark');
  });

  it('reports the system preference when matchMedia is absent', () => {
    vi.stubGlobal('matchMedia', undefined);
    expect(systemTheme()).toBe('light');
  });
});
