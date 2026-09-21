import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

/**
 * Two rules in this stylesheet that no rendered test can reach: jsdom applies
 * no cascade to a sheet it never loads and resolves no media query, so both
 * defects below were invisible to the suite and were found in a browser.
 */

const CSS = readFileSync(
  fileURLToPath(new URL('./components.css', import.meta.url)),
  'utf8',
);

/** `/* … *\/` removed so a commented-out rule cannot satisfy a check. */
function withoutComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, '');
}

const BARE = withoutComments(CSS);

const LAYOUT = withoutComments(
  readFileSync(fileURLToPath(new URL('./layout.css', import.meta.url)), 'utf8'),
);

function mediaBlock(source: string, query: string, needle: string): string {
  let at = 0;
  for (;;) {
    const start = source.indexOf(query, at);
    expect(start, `no ${query} block mentions ${needle}`).toBeGreaterThan(-1);
    const end = source.indexOf('\n}', start);
    const block = source.slice(start, end === -1 ? undefined : end);
    if (block.includes(needle)) return block;
    at = start + query.length;
  }
}

describe('the blocked-share fills', () => {
  /**
   * `.bar > span` sets the accent and is one type selector more specific than
   * a bare class, so `.c-ratio-fill { background: var(--series-blocked) }` lost
   * the cascade and every "blocked share" bar was painted with the *permitted*
   * hue — measured `rgb(31,157,187)` where the artboards draw `#d1504b`.
   */
  it('are qualified by their track, or they lose to `.bar > span`', () => {
    for (const fill of ['c-ratio-fill', 'policy-traffic-fill']) {
      expect(BARE).toContain(`.bar > span.${fill}`);
      expect(BARE).not.toMatch(
        new RegExp(`(?:^|[,}\\s])\\.${fill}\\s*\\{`, 'm'),
      );
    }
  });
});

describe('the Diagnostics group in the icon rail', () => {
  it('shows its members and hides the head, since `.sb .sub2` is display:none there and a disclosure would open nothing', () => {
    expect(LAYOUT).toMatch(/\n\.it-rail-child \{\s*display: none;/);

    const rail = mediaBlock(LAYOUT, '@media (max-width: 1199px)', '.sub2');
    expect(rail).toMatch(
      /\.sb \.sub2,\s*\.sb \.it-group-wide \{\s*display: none;/,
    );
    expect(rail).toMatch(/\.sb \.it-rail-child \{\s*display: flex;/);

    const drawer = mediaBlock(LAYOUT, '@media (max-width: 767px)', '.sub2');
    expect(drawer).toMatch(
      /\.sb \.sub2,\s*\.sb \.it-group-wide \{\s*display: flex;/,
    );
    expect(drawer).toMatch(/\.sb \.it-rail-child \{\s*display: none;/);
  });
});

describe('the Clients family filter', () => {
  it('is rendered twice and hidden once, so `.ch .chips` cannot take the page’s only re-read off the phone', () => {
    expect(BARE).toMatch(/\.ch \.chips \{\s*display: none;/);
    expect(BARE).toMatch(/\n\.chips-mobile \{\s*display: none;/);
    expect(BARE).toMatch(
      /\.card:has\(\.clients-table\) \.chips-mobile \{\s*display: flex;/,
    );
    expect(BARE).toMatch(
      /\.card:has\(\.clients-table\) \.ch \.chips-mobile \.chips \{\s*display: flex;/,
    );
  });

  it('sizes the phone chips from their own words, since `flex: 1` is a zero basis that collapses them in a title row', () => {
    expect(BARE).toMatch(
      /\.card:has\(\.clients-table\) \.chips-mobile \.chip \{\s*flex: 0 0 auto;/,
    );
  });
});

describe('touch targets', () => {
  /**
   * The 44 px rules live in the `≤ 767 px` blocks, so a tablet between 768 and
   * 1199 px took the desktop layout and the desktop sizes with it: measured at
   * 768 px and 1024 px — both iPad orientations — `Edit` and `Delete` arrived
   * 41.3 × 25.6 and the client search box 34 tall. Width is not the question;
   * the pointer is.
   */
  it('are raised on a coarse pointer, not only on a narrow viewport', () => {
    const block = /@media \(pointer: coarse\) \{([\s\S]*?)\n\}/.exec(BARE);
    expect(block).not.toBeNull();
    const body = block?.[1] ?? '';
    for (const selector of ['.btn', '.search-input', '.policy-tools .btn']) {
      expect(body).toContain(selector);
    }
    expect(body).toContain('min-height: 44px');
  });
});
