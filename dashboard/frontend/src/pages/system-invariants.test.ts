import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * The properties of the System screens that only reading the source can show.
 * A rendered test cannot prove that a string exists once rather than twice, or
 * that a module was never imported — and both are acceptance criteria here.
 *
 * Same enforcement shape as `lifecycle/timers.test.ts` and
 * `pages/filtering-invariants.test.ts`, and stated the same way: this is test
 * enforcement, not an architectural guarantee.
 */

const ROOT = join(import.meta.dirname, '..');

function sourcesUnder(directory: string, prefix: string): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const entry of readdirSync(directory)) {
    const path = join(directory, entry);
    const relative = prefix === '' ? entry : `${prefix}/${entry}`;
    if (statSync(path).isDirectory()) {
      out.push(...sourcesUnder(path, relative));
      continue;
    }
    if (!/\.tsx?$/.test(entry) || /\.test\.tsx?$/.test(entry)) continue;
    out.push([relative, readFileSync(path, 'utf8')]);
  }
  return out;
}

const SOURCES = sourcesUnder(ROOT, '');

/** Prose is not a call. Both diagnostics pages explain in a doc comment that
 *  the socket is closed while they are the active route, which is exactly the
 *  absence being asserted. */
function withoutComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '');
}

/** The four screens this task ships, and only those — the Dashboard and Lists
 *  hold subscriptions of their own and are not in question here. */
const SYSTEM_PAGES: Array<[string, string]> = [
  'pages/settings.tsx',
  'pages/diagnostics-health.tsx',
  'pages/diagnostics-memory.tsx',
  'pages/live-feed.tsx',
].map((name) => [
  name,
  withoutComments(SOURCES.find(([file]) => file === name)?.[1] ?? ''),
]);

describe('the degraded explanation', () => {
  /**
   * "Degraded status is explained identically wherever it appears — one string
   * in the code, not three." Upstreams raised it in p5-08 and Health shows the
   * same reading; the component moved to `components/` rather than being
   * restated, and this is what keeps a fourth copy from appearing.
   */
  it('exists exactly once in the source', () => {
    const phrase = 'penalized or being probed';
    const holders = SOURCES.filter(([, text]) => text.includes(phrase)).map(
      ([name]) => name,
    );
    expect(holders).toEqual(['components/degraded-banner.tsx']);
  });

  it('is reached by both pages through that one module', () => {
    const importers = SOURCES.filter(([name, text]) =>
      /^pages\//.test(name) && text.includes("components/degraded-banner"),
    ).map(([name]) => name);
    expect(importers.sort()).toEqual([
      'pages/diagnostics-health.tsx',
      'pages/upstreams.tsx',
    ]);
  });
});

describe('what the System pages subscribe to', () => {
  /**
   * Health and Memory must hold **no** event subscription: the socket is closed
   * while either is the active route, which is what makes "an inactive page has
   * approximately zero API activity attributable to it" true for them. The
   * route table declares that; this is what proves no page reached past it.
   */
  it('is `config_changed` on Settings and `query` on the Live Feed, and nothing else', () => {
    const listeners = SYSTEM_PAGES.filter(([, text]) =>
      text.includes('socket.on('),
    ).map(([name]) => name);
    expect(listeners.sort()).toEqual(['pages/live-feed.tsx', 'pages/settings.tsx']);
  });

  it('leaves Health and Memory with no socket reference at all', () => {
    for (const page of ['pages/diagnostics-health.tsx', 'pages/diagnostics-memory.tsx']) {
      const source = SYSTEM_PAGES.find(([name]) => name === page)?.[1] ?? '';
      expect(source, page).not.toBe('');
      // Comments stripped: both pages *explain* that the socket is closed while
      // they are the active route, which is prose about an absence.
      expect(source, page).not.toContain('socket');
    }
  });

  it('subscribes to `query` in exactly one page', () => {
    const holders = SOURCES.filter(
      ([name, text]) => /^pages\//.test(name) && text.includes("socket.on('query'"),
    ).map(([name]) => name);
    expect(holders).toEqual(['pages/live-feed.tsx']);
  });
});

describe('the Live Feed’s own machinery', () => {
  /**
   * The feed coalesces onto an animation frame, and that token belongs to
   * `lifecycle/timers.ts` — `lifecycle/timers.test.ts` enforces it globally.
   * This states the narrower fact the task asks for: the feed's tree reaches a
   * scheduler only through that module's `onNextFrame`.
   */
  it('reaches a frame only through the timers module', () => {
    const tree = SOURCES.filter(([name]) => /^pages\/live-feed/.test(name));
    expect(tree.length).toBeGreaterThan(1);
    for (const [name, text] of tree) {
      for (const token of ['setTimeout', 'setInterval', 'requestAnimationFrame']) {
        expect(text, `${name}: ${token}`).not.toContain(token);
      }
    }
    const ring = tree.find(([name]) => name === 'pages/live-feed/ring.ts')?.[1] ?? '';
    expect(ring).toContain("from '../../lifecycle/timers'");
  });
});
