import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * The four filtering routes are the phase's cheapest screens by construction:
 * no event subscription, no timer, no chart, no clock. Three of those four are
 * absences, and an absence is only provable by reading the source — a rendered
 * test cannot show that a module was never imported.
 *
 * This is the same enforcement shape as `lifecycle/timers.test.ts`: test
 * enforcement, not an architectural guarantee, and it is stated as such.
 */

const ROOT = join(import.meta.dirname, '..');

const PAGES = [
  'pages/rules.tsx',
  'pages/policies.tsx',
  'pages/clients.tsx',
  'pages/rule-tester.tsx',
];

const DIRECTORIES = [
  'pages/rules',
  'pages/policies',
  'pages/clients',
  'pages/rule-tester',
  'policy',
];

function sourcesUnder(relative: string): string[] {
  const { readdirSync, statSync } = require('node:fs') as typeof import('node:fs');
  const full = join(ROOT, relative);
  if (!statSync(full).isDirectory()) return [full];
  return readdirSync(full)
    .filter((name) => /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name))
    .map((name) => join(full, name));
}

function everySource(): Array<[string, string]> {
  const files = [
    ...PAGES.map((path) => join(ROOT, path)),
    ...DIRECTORIES.flatMap(sourcesUnder),
  ];
  return files.map((file) => [file, readFileSync(file, 'utf8')]);
}

describe('what the four filtering routes never reach for', () => {
  it('imports no chart module, so none of them pulls the uPlot chunk', () => {
    for (const [file, source] of everySource()) {
      expect(source, file).not.toMatch(/from '.*charts\//);
      expect(source, file).not.toMatch(/from '.*components\/chart'/);
      expect(source, file).not.toMatch(/uplot/i);
    }
  });

  it('subscribes to no event and to no polled endpoint', () => {
    for (const [file, source] of everySource()) {
      expect(source, file).not.toContain('useRefresh');
      expect(source, file).not.toContain('RefreshCluster');
      expect(source, file).not.toContain('socket.on');
      expect(source, file).not.toContain('refresh.invalidate');
    }
  });

  // `nowMs()` is the one permitted use of the timer module: a `last_seen`
  // label is computed once per render from the fetched payload. A ticker that
  // re-renders itself is a timer, and these pages have none.
  it('schedules nothing, and runs no age ticker', () => {
    for (const [file, source] of everySource()) {
      expect(source, file).not.toContain('subscribeAgeTick');
      // The bare call, not `.every(` / `.after(` — `lifecycle/timers.ts`
      // exports the two schedulers as free functions.
      expect(source, file).not.toMatch(/(?<![.\w])every\(/);
      expect(source, file).not.toMatch(/(?<![.\w])after\(/);
    }
  });

  it('listens to no viewport, because the phone layout is CSS', () => {
    for (const [file, source] of everySource()) {
      expect(source, file).not.toContain('matchMedia');
      expect(source, file).not.toContain('innerWidth');
      expect(source, file).not.toContain('ResizeObserver');
    }
  });

  // D3 and D4: no schedule window is evaluated against a clock anywhere. The
  // "in force" and "window shut" statements are read off `client.policy`,
  // which is the server's own answer.
  it('does no date arithmetic inside `src/policy/`', () => {
    for (const file of sourcesUnder('policy')) {
      // Comments are stripped first: one of them names `toLocaleLowerCase` to
      // say why the fold is an explicit ASCII one, which is the opposite of
      // the defect this guards against.
      const source = readFileSync(file, 'utf8').replace(
        /\/\*[\s\S]*?\*\//g,
        '',
      );
      expect(source, file).not.toContain('Date');
      expect(source, file).not.toContain('toLocale');
      expect(source, file).not.toContain('getTimezoneOffset');
    }
  });

  // The endpoint exists and is the single-address read and write path; no page
  // here reads it, and `api/clients.ts` exposes no accessor for the GET.
  it('has no accessor for the per-address policy read', () => {
    const source = readFileSync(join(ROOT, 'api/clients.ts'), 'utf8');
    expect(source).not.toContain('getClientPolicy');
  });
});
