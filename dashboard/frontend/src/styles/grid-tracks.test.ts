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

/** The declared value, or `null` when the rule does not set that property. */
function declared(selector: string, property: string): string | null {
  const source = withoutComments(CSS);
  const at = source.indexOf(selector);
  expect(at, `${selector} not found`).toBeGreaterThan(-1);
  const open = source.indexOf('{', at);
  const close = source.indexOf('}', open);
  const block = source.slice(open + 1, close);
  const match = new RegExp(`(?:^|;)\\s*${property}\\s*:([^;]+)`).exec(block);
  return match === null ? null : (match[1] ?? '').replace(/\s+/g, ' ').trim();
}

function declaration(selector: string, property: string): string {
  const value = declared(selector, property);
  expect(value, `${property} not found in ${selector}`).not.toBeNull();
  return value ?? '';
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

  it('is exactly the three 44 px action targets wide', () => {
    // The actions are glyphs, so the cell is 3 × 44 px in every row state and
    // the track is that figure — not a clearance above the widest label, which
    // is what it had to be while they were words (130 / 186 / 189 px by state).
    // Tiled, never overlapping: adjacent 44 px targets that overlap let the
    // topmost take its neighbour's taps.
    const last = Number.parseInt(tracks[tracks.length - 1] ?? '0', 10);
    expect(last).toBe(3 * 44);
  });

  it('floors narrow enough to fit the smallest desktop width', () => {
    // The row cannot be narrower than the sum of its track minimums, its seven
    // gaps and its padding, and that sum is what decides whether the table
    // needs an internal scrollbar. Measured: the card gives the table the
    // viewport less 271 px of chrome, so at visual-system.md §Responsive's
    // 1200 px desktop breakpoint there are 912 px to fit into.
    //
    // With text actions the floor was 958 px and the table scrolled at every
    // desktop width below 1247 px. Glyph actions took the last track from
    // 200 px to 132 px and the floor to 890, which clears 912 — so the table
    // fits across the whole desktop range and no exception has to be
    // documented. Pinned: anything above 912 brings that band back.
    const GAPS = 7 * 10;
    const ROW_PADDING = 2 * 14;
    const DESKTOP_CARD_PX = 1200 - 271 - 17;
    const floor = tracks
      .join(' ')
      .replace(/minmax\(\s*(\d+)px[^)]*\)/g, '$1px')
      .split(' ')
      .filter((token) => token.endsWith('px'))
      .reduce((sum, token) => sum + Number.parseInt(token, 10), 0);
    expect(floor + GAPS + ROW_PADDING).toBe(890);
    expect(floor + GAPS + ROW_PADDING).toBeLessThanOrEqual(DESKTOP_CARD_PX);
  });

  it('gives the Rules column the largest share of the slack', () => {
    // Its minimum is already above its proportional share, so under equal
    // weights every pixel of slack went to List and Status and the mono
    // partition line wrapped to three lines at every desktop width. Weight,
    // not floor — this costs no width at all.
    const weights = [...tracks.join(' ').matchAll(/([\d.]+)fr/g)].map((hit) =>
      Number.parseFloat(hit[1] ?? '0'),
    );
    expect(weights).toHaveLength(3);
    const rules = weights[2] ?? 0;
    expect(rules).toBeGreaterThan(weights[0] ?? 0);
    expect(rules).toBeGreaterThan(weights[1] ?? 0);
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
    // The eight columns stop fitting well above 1199 px — measured 1094 px of
    // table in a 1062 px container at a 1333 px viewport. visual-system.md
    // §Responsive puts that scroll inside the table's own container; the page
    // body never scrolls sideways at any width. Measured before the rule
    // existed: 964 px of content in an 883 px viewport on `/lists`.
    expect(declaration('.lists-table', 'overflow-x')).toBe('auto');
  });

  it('does not make the card body a scroll container', () => {
    // A horizontal scroll container is a scroll container on both axes: CSS
    // computes a `visible` companion to `auto`, and `clip` computes to
    // `hidden` in the same position, so one axis alone cannot be asked for.
    // The smallest element that can carry it therefore should. On the card
    // body it also swept in the footnote, which is prose and belongs wrapped
    // rather than scrolled sideways with the rows.
    expect(declared('.bd.lists-body', 'overflow-x')).toBeNull();
    expect(declared('.bd.lists-body', 'overflow-y')).toBeNull();
    expect(declared('.bd.lists-body', 'overflow')).toBeNull();
  });

  it('never constrains the scroller’s height, so nothing scrolls vertically', () => {
    // The vertical axis of that scroller is `auto` whether or not it is
    // declared. It stays inert only while the container's height is its
    // content's — a `height` or `max-height` here would turn the spec side
    // effect into a real scrollbar inside the card.
    expect(declared('.lists-table', 'height')).toBeNull();
    expect(declared('.lists-table', 'max-height')).toBeNull();
  });

  it('floors each row at its own tracks inside that scroller', () => {
    // On the rows, not on the container: the container has to be free to be
    // the scrollport, and a floor on it would push the scroll onto the page.
    expect(declaration('.list-head,\n  .list-row', 'min-width')).toBe(
      'min-content',
    );
  });
});

describe('the Lists row’s Status cell', () => {
  it('breaks a bare URL rather than painting outside its column', () => {
    // `last_error` is a server string and routinely carries a URL, which has no
    // break opportunity. Measured at the column's floor before this rule: a
    // 145 px token in a 110 px cell, 35 px of it in the gutter beside it.
    expect(declaration('.l-status .note', 'overflow-wrap')).toBe('anywhere');
  });
});

describe('the list toggle', () => {
  it('keeps the artboard’s 32 × 18 pill', () => {
    expect(declaration('.switch {', 'width')).toBe('32px');
    expect(declaration('.switch {', 'height')).toBe('18px');
  });

  it('gives it a 44 × 44 target regardless', () => {
    // An 18 px-tall control cannot meet the task's "at least 44 px", and the
    // pill is the artboard's figure — so the target grows and the drawing does
    // not. Centred on the pill, so the box stays 44 px whatever the pill is.
    expect(declaration('.switch::before', 'width')).toBe('44px');
    expect(declaration('.switch::before', 'height')).toBe('44px');
    expect(declaration('.switch::before', 'position')).toBe('absolute');
  });
});
