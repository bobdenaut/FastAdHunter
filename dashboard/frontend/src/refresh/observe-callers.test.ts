import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * `RefreshRegistry.observe` is a passive tap with exactly one caller, and that
 * is the property that keeps it from becoming a second polling mechanism. Its
 * unit tests prove it starts no timer and issues no fetch; only reading the
 * source can prove nothing else reaches for it.
 *
 * Same enforcement shape as `lifecycle/timers.test.ts` and
 * `styles/literal-colours.test.ts`: an **allowlist**, so widening it is a
 * deliberate edit with a reason beside it rather than a call that slipped in.
 */

const ROOT = join(import.meta.dirname, '..');

/**
 * | file | why |
 * | ---- | --- |
 * | `services.ts` | the wiring — the restart banner's `/health` tap |
 * | `components/chart.tsx` | `ResizeObserver.observe(node)`, an unrelated DOM API that spells the same word |
 */
const ALLOWED = ['components/chart.tsx', 'services.ts'];

/** Prose naming the method is not a call — `restart-banner.ts`' own header
 *  explains where its readings come from and would otherwise read as one. */
function withoutComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '');
}

function sources(directory: string, prefix: string): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const entry of readdirSync(directory)) {
    const path = join(directory, entry);
    const relative = prefix === '' ? entry : `${prefix}/${entry}`;
    if (statSync(path).isDirectory()) {
      out.push(...sources(path, relative));
      continue;
    }
    if (!/\.tsx?$/.test(entry) || /\.test\.tsx?$/.test(entry)) continue;
    out.push([relative, withoutComments(readFileSync(path, 'utf8'))]);
  }
  return out;
}

describe('who may call `.observe`', () => {
  it('is `services.ts`, plus the one DOM observer that shares the name', () => {
    const callers = sources(ROOT, '')
      .filter(([, text]) => text.includes('.observe('))
      .map(([name]) => name)
      .sort();
    expect(callers).toEqual(ALLOWED);
  });

  it('narrows the registry to `observe` where it is handed over', () => {
    const services = readFileSync(join(ROOT, 'services.ts'), 'utf8');
    expect(services).toContain("Pick<RefreshRegistry, 'observe'>");
    // The narrowing is only worth anything if the tap cannot reach the two
    // methods that would make it a poller.
    const tap = services.slice(services.indexOf('const healthTap'));
    expect(tap).not.toContain('healthTap.subscribe');
    expect(tap).not.toContain('healthTap.invalidate');
  });

  it('reads the observer map in exactly two places in the registry', () => {
    const registry = readFileSync(join(ROOT, 'refresh/registry.ts'), 'utf8');
    // `observe()` writes it; `announce()` reads it. A third site — in
    // `subscribe`, `startTimer`, `fetch`, `isStale` or `setSuspended` — is the
    // moment this stops being a tap.
    const uses = registry.match(/this\.observers/g) ?? [];
    expect(uses).toHaveLength(3);
    expect(registry).toContain('for (const tap of this.observers.get(endpoint)');
  });
});
