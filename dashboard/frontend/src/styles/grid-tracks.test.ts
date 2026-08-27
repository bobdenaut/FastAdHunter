import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

/**
 * The one invariant in this stylesheet that no rendered test can reach.
 *
 * jsdom has no layout engine, so `getComputedStyle().gridTemplateColumns`
 * returns the declaration rather than the resolved tracks — a misaligned grid
 * is invisible to every other test in this suite. The defect it guards against
 * shipped once: the Lists table's last track was `minmax(0, auto)`, and because
 * each row is its own grid that track was measured **per row** from that row's
 * action labels (130 px normally, 186 px while a refresh was pending, 189 px on
 * a `rejected` row). The difference came off the three `fr` tracks, so the
 * header sat up to 130 px right of its own column and a row's other five cells
 * jumped sideways the moment its Refresh was clicked.
 *
 * Reading the source is therefore the honest test: the header and the rows are
 * two separate grids, and two separate grids agree only if every track is
 * content-independent.
 */

const CSS = readFileSync(
  fileURLToPath(new URL('./components.css', import.meta.url)),
  'utf8',
);

/** `/* … *\/` removed so a commented-out declaration cannot satisfy a check. */
function withoutComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, '');
}

function declaration(selector: string, property: string): string {
  const source = withoutComments(CSS);
  const at = source.indexOf(selector);
  expect(at, `${selector} not found`).toBeGreaterThan(-1);
  const open = source.indexOf('{', at);
  const close = source.indexOf('}', open);
  const block = source.slice(open + 1, close);
  const match = new RegExp(`(?:^|;)\\s*${property}\\s*:([^;]+)`).exec(block);
  expect(match, `${property} not found in ${selector}`).not.toBeNull();
  return (match?.[1] ?? '').replace(/\s+/g, ' ').trim();
}

describe('the Lists table grid', () => {
  const tracks = declaration('.list-head,\n.list-row', 'grid-template-columns')
    .split(' ')
    .filter((token) => token !== '');

  it('is eight columns, matching the artboard', () => {
    // List · On · Every · Last refresh · Status · Rules · Total · actions.
    // `minmax(a, b)` survives the split as three tokens, so count the
    // separators rather than the tokens.
    const columns = tracks
      .join(' ')
      .replace(/minmax\([^)]*\)/g, 'T')
      .split(' ')
      .filter((token) => token !== '');
    expect(columns).toHaveLength(8);
  });

  it('sizes its actions column from nothing the content decides', () => {
    const last = tracks[tracks.length - 1];
    expect(last).toMatch(/^\d+px$/);
  });

  it('leaves the widest action set room', () => {
    // Measured in a browser at the shipped type scale: `Refresh · Edit ·
    // Remove` is 130 px, `refresh requested · …` 186 px and
    // `Delete and re-add · …` 189 px. A longer label than any of those needs
    // this figure raised with it.
    const last = Number.parseInt(tracks[tracks.length - 1] ?? '0', 10);
    expect(last).toBeGreaterThanOrEqual(189);
  });

  it('has no other content-sized track', () => {
    // `auto`, `min-content`, `max-content` and `fit-content` all resolve from
    // what a row happens to hold, which is what made the two grids disagree.
    // `fr` is safe here: it divides the *leftover*, which is identical in both
    // grids once nothing else is content-sized.
    const template = tracks.join(' ');
    expect(template).not.toMatch(/\b(auto|min-content|max-content|fit-content)\b/);
  });
});

describe('the Lists row’s Rules cell', () => {
  it('does not print the tier figures a second time', () => {
    // `StageBar` renders its own swatch legend beside every bar. The card title
    // bar already carries that legend once for the whole table
    // (`Lists.dc.html`), and the mono partition line under the bar already
    // carries the three figures — so the row's copy was the same numbers twice
    // and two lines of the narrowest cell on the row.
    expect(declaration('.l-rules .seg-legend', 'display')).toBe('none');
  });
});

describe('the Lists table container', () => {
  it('scrolls the table rather than the page body', () => {
    // Eight columns stop fitting between 768 and 1199 px. visual-system.md
    // §Responsive puts that scroll inside the table's own container; the page
    // body never scrolls sideways at any width. Measured before this rule
    // existed: 964 px of content in an 883 px viewport on `/lists`.
    expect(declaration('.bd.lists-body', 'overflow-x')).toBe('auto');
  });

  it('floors the grid at its own tracks inside that scroller', () => {
    expect(declaration('.lists-table', 'min-width')).toBe('min-content');
  });
});
