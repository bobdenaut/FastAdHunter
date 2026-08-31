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

  it('is nine columns', () => {
    // List · On · Every · Last refresh · Next refresh · Status · Rules · Total
    // · actions. `minmax(a, b)` survives the split as three tokens, so count
    // the separators rather than the tokens.
    const columns = tracks
      .join(' ')
      .replace(/minmax\([^)]*\)/g, 'T')
      .split(' ')
      .filter((token) => token !== '');
    expect(columns).toHaveLength(9);
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
    // The row cannot be narrower than the sum of its track minimums, its eight
    // gaps and its padding, and that sum is what decides whether the table
    // needs an internal scrollbar. Measured: the card gives the table the
    // viewport less 271 px of chrome, so at visual-system.md §Responsive's
    // 1200 px desktop breakpoint there are 912 px to fit into.
    //
    // With text actions the floor was 958 px and the table scrolled at every
    // desktop width below 1247 px. Glyph actions took the last track from
    // 200 px to 132 px and the floor to 890, which cleared 912 — so the table
    // fits across the whole desktop range and no exception has to be
    // documented. Pinned: anything above 912 brings that band back.
    //
    // The Next refresh column and its gap cost 72 px of that clearance, and
    // Status paid for it: it was `minmax(110px, 1fr)` where the widest pill it
    // can hold — `degraded` — measures 73 px, so it is a fixed 78 px now and
    // the failure prose beneath the pill wraps a little sooner. `Every` gave
    // up the last 2 px. Measured at 1200 px on the built page: the row is
    // 913 px in a 913 px card, spilling nothing.
    const GAPS = 8 * 10;
    const ROW_PADDING = 2 * 14;
    const DESKTOP_CARD_PX = 1200 - 271 - 17;
    const floor = tracks
      .join(' ')
      .replace(/minmax\(\s*(\d+)px[^)]*\)/g, '$1px')
      .split(' ')
      .filter((token) => token.endsWith('px'))
      .reduce((sum, token) => sum + Number.parseInt(token, 10), 0);
    expect(floor + GAPS + ROW_PADDING).toBe(912);
    expect(floor + GAPS + ROW_PADDING).toBeLessThanOrEqual(DESKTOP_CARD_PX);
  });

  it('gives the Rules column the largest share of the slack', () => {
    // Its minimum is already above its proportional share, so under equal
    // weights every pixel of slack went to List and Status and the mono
    // partition line wrapped to three lines at every desktop width. Weight,
    // not floor — this costs no width at all.
    //
    // Two weights, not three: Status stopped taking a share when it was fixed
    // at the width of its widest pill, so the slack is now split between List
    // and Rules alone.
    const weights = [...tracks.join(' ').matchAll(/([\d.]+)fr/g)].map((hit) =>
      Number.parseFloat(hit[1] ?? '0'),
    );
    expect(weights).toHaveLength(2);
    const rules = weights[1] ?? 0;
    expect(rules).toBeGreaterThan(weights[0] ?? 0);
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
    // The eight columns stop fitting below the 1200 px desktop breakpoint —
    // measured on the final tree, 890 px of table in an 878 px container at a
    // 1000 px viewport. visual-system.md §Responsive puts that scroll inside
    // the table's own container; the page body never scrolls sideways at any
    // width. Measured before the rule existed: 964 px of content in an 883 px
    // viewport on `/lists`. (With the text actions this rule replaced, the
    // floor was 958 px and the scroll reached up to 1247 px; the glyphs took it
    // to 890 and the case above pins that it stays under the breakpoint's 912.)
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

/**
 * p5-07's Clients table. `p5-06`'s F1 was two grids given the same template,
 * and they drifted; this is **one** grid — the rows are `subgrid` children of
 * the table's tracks — so there is a single definition and the header cannot
 * disagree with a row. jsdom still resolves no layout, so the source is again
 * the honest test.
 */
describe('the Clients table grid', () => {
  const tracks = declaration('.clients-table', 'grid-template-columns');

  it('is eight columns, matching the artboard', () => {
    const columns = tracks
      .replace(/minmax\([^)]*\)/g, 'T')
      .split(' ')
      .filter((token) => token !== '');
    expect(columns).toHaveLength(8);
  });

  it('sizes its actions column from nothing the content decides', () => {
    const columns = tracks
      .replace(/minmax\([^)]*\)/g, 'T')
      .split(' ')
      .filter((token) => token !== '');
    expect(columns[columns.length - 1]).toBe('44px');
  });

  it('gives the header and the rows one grid rather than two that agree', () => {
    expect(
      declaration('.client-head,\n.client-row', 'grid-template-columns'),
    ).toBe('subgrid');
    expect(declaration('.client-head,\n.client-row', 'grid-column')).toBe(
      '1 / -1',
    );
  });

  it('scrolls the table inside its own container, never the page body', () => {
    expect(declaration('.clients-scroll', 'overflow-x')).toBe('auto');
    expect(declaration('.clients-table', 'min-width')).toBe('min-content');
  });
});

describe('the row action glyph', () => {
  it('is a 44 × 44 target on every page that uses it', () => {
    expect(declaration('.iconbtn', 'width')).toBe('44px');
    expect(declaration('.iconbtn', 'height')).toBe('44px');
  });
});

describe('the dashed policy chip', () => {
  // p5-06's F17/N4: a tint that reads in one theme and vanishes in the other.
  // `color-mix(… X%, transparent)` premultiplies to about 1.8 % alpha, so the
  // difference is carried by border style, weight and the words instead.
  it('differs by border style and weight, never by a transparent wash', () => {
    expect(declaration('.pchip.inh', 'border-style')).toBe('dashed');
    expect(declaration('.pchip', 'font-weight')).toBe('500');
    expect(declaration('.pchip.inh', 'font-weight')).toBe('400');
    expect(declaration('.pchip.inh', 'background')).toBe('var(--surface)');
  });
});

/**
 * p5-08's grids. Same reasoning as above — jsdom resolves no layout, so the
 * declarations are read as text; the browser widths they produce are measured
 * in the task's verification pass instead.
 */
/**
 * The `@media` block matching `query` that mentions `needle`. There are several
 * blocks per breakpoint — the sheet is grouped by page rather than by width —
 * so a bare `indexOf` on the query finds whichever came first.
 */
function mediaBlockWith(query: string, needle: string): string {
  const source = withoutComments(CSS);
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

describe('the p5-08 four-up grids', () => {
  it('folds the upstream counter grid to two columns on a phone', () => {
    expect(declaration('.ep-counters {', 'grid-template-columns')).toBe(
      'repeat(4, minmax(0, 1fr))',
    );
    expect(
      mediaBlockWith('@media (max-width: 767px)', '.ep-counters'),
    ).toMatch(/\.ep-counters \{\s*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\);/);
  });

  it('folds the cache stage legend the same way', () => {
    expect(declaration('.stage-notes {', 'grid-template-columns')).toBe(
      'repeat(4, minmax(0, 1fr))',
    );
    expect(mediaBlockWith('@media (max-width: 767px)', '.stage-notes')).toMatch(
      /\.stage-notes \{\s*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\);/,
    );
  });

  it('gives the endpoint row three zones that stack down to one', () => {
    // Three across on a desktop; two at the tablet width with the histogram on
    // its own full-width line; one card per endpoint on a phone.
    expect(declaration('.ep {', 'grid-template-columns')).toBe(
      'minmax(0, 1.1fr) minmax(0, 2fr) minmax(0, 0.9fr)',
    );
    const tablet = mediaBlockWith('@media (max-width: 1199px)', '.ep-runs');
    expect(tablet).toMatch(
      /\.ep \{\s*grid-template-columns: minmax\(0, 1fr\) minmax\(0, 1\.6fr\);/,
    );
    expect(tablet).toMatch(/\.ep-runs \{\s*grid-column: 1 \/ -1;/);
    expect(mediaBlockWith('@media (max-width: 767px)', '.ep-counters')).toMatch(
      /\.ep \{\s*grid-template-columns: minmax\(0, 1fr\);/,
    );
  });
});

describe('the phone Cache card (p5-06 F5, closed by p5-08)', () => {
  it('drops the `free` band and its legend entry below 768 px', () => {
    // `MobileDashboard.dc.html` draws three segments and three legend entries.
    // A selector rather than a conditional prop: this application has no
    // viewport listener anywhere, and one band on one card does not justify
    // introducing the first.
    expect(
      declaration(
        '.z-cache .seg > div:nth-child(4),\n  .z-cache .seg-legend > span:nth-child(4)',
        'display',
      ),
    ).toBe('none');
  });

  it('keeps all four bands on the Cache page itself', () => {
    // The page whose subject is the cache states every stage, including the
    // free one — the phone omission is the Dashboard summary's, not the
    // figure's.
    const source = withoutComments(CSS);
    expect(source).not.toMatch(/\.cache-stage \.seg > div:nth-child\(4\)/);
  });
});

describe('the p5-08 tinted surfaces', () => {
  it('tints a penalized row and the degraded banner from theme-paired tokens', () => {
    // p5-06's F17/N4: a `color-mix(…, transparent)` wash premultiplies to
    // about 2 % alpha and vanishes in one of the two themes. Both of these are
    // token pairs that exist in every palette block.
    expect(declaration('.ep.is-penalized', 'background')).toBe(
      'var(--pill-warn-bg)',
    );
    expect(declaration('.banner.warn', 'background')).toBe('var(--pill-warn-bg)');
  });

  it('tints the cache callout the same way', () => {
    expect(declaration('.callout', 'background')).toBe('var(--surface-sunken)');
  });
});

describe('the p5-08 touch targets', () => {
  it('gives the clean action’s choice a 44 px row', () => {
    // The checkbox the artboard draws is 15 px. The target grows and the
    // drawing does not — the same rule the list toggle already follows.
    expect(declaration('.clean-choice', 'min-height')).toBe('44px');
  });
});

describe('the p5-08 chart legends', () => {
  it('scopes its wrapping to this page’s legends, not to `.chart-legend`', () => {
    // The base rule is p5-06's and the Dashboard's own legend renders through
    // it. Re-opening `.chart-legend` from the Performance section changed a
    // p5-06 page from a page that does not name it.
    const rules = withoutComments(CSS).match(/^\.chart-legend \{/gm) ?? [];
    expect(rules, '`.chart-legend` is declared more than once').toHaveLength(1);
    expect(
      declaration('.latency-legend,\n.verdicts-legend', 'flex-wrap'),
    ).toBe('wrap');
    expect(
      declaration('.latency-legend,\n.verdicts-legend', 'align-items'),
    ).toBe('baseline');
  });
});

/**
 * Every `≤ 767 px` block's body, joined. There is more than one of them —
 * `p5-06` opened the first and each later task has added its own — so slicing
 * from the last occurrence reads whichever block happens to be last in the
 * file, which is how four of these checks silently started reading the wrong
 * one. Braces are balanced rather than matched by regex, because the blocks
 * hold nested rules.
 */
function phoneRules(): string {
  const source = withoutComments(CSS);
  const marker = '@media (max-width: 767px)';
  const bodies: string[] = [];
  let at = source.indexOf(marker);
  while (at !== -1) {
    let depth = 0;
    let cursor = source.indexOf('{', at);
    const start = cursor + 1;
    for (; cursor < source.length; cursor += 1) {
      if (source[cursor] === '{') depth += 1;
      else if (source[cursor] === '}') {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    bodies.push(source.slice(start, cursor));
    at = source.indexOf(marker, cursor);
  }
  expect(bodies.length, 'no phone block found').toBeGreaterThan(0);
  return bodies.join('\n');
}

describe('the p5-09 grids', () => {
  it('gives Settings a label column and a control column, folding to one', () => {
    // "Settings fields stack label-over-control" below 768 px is the task's
    // wording, and jsdom resolves no media query — so the declaration is read.
    expect(declaration('.set-field', 'grid-template-columns')).toBe(
      '260px minmax(0, 1fr)',
    );
    const phone = phoneRules();
    expect(phone).toContain('.set-field {');
    expect(phone).toMatch(/\.set-field \{[^}]*minmax\(0, 1fr\)/);
  });

  it('sizes the upstream row from nothing its content decides', () => {
    // Index, protocol and the remove target are fixed tracks; the two text
    // cells share the slack. A content-sized track would make every row a
    // different width, because each row is its own grid.
    const tracks = declaration('.upstream-row {', 'grid-template-columns');
    expect(tracks).toBe('28px minmax(0, 1fr) 92px minmax(0, 1fr) 44px');
    // The remove glyph is a 44 px target, tiled rather than overlapping.
    expect(tracks.endsWith('44px')).toBe(true);
  });

  it('gives the raw panel a key column that cannot be pushed out', () => {
    expect(declaration('.raw-row {', 'grid-template-columns')).toBe(
      'minmax(0, 320px) minmax(0, 1fr)',
    );
  });

  it('folds the health problem row to one column on a phone', () => {
    expect(declaration('.health-problem {', 'grid-template-columns')).toBe(
      '160px 90px minmax(0, 1fr)',
    );
    const phone = phoneRules();
    expect(phone).toMatch(/\.health-problem \{[^}]*minmax\(0, 1fr\)/);
  });

  it('scrolls the feed table inside its own container, never the page body', () => {
    expect(declared('.feed-scroll', 'overflow-x')).toBe('auto');
    // And the table has a floor, so the nine columns cannot be squeezed into
    // an unreadable width instead of scrolling.
    expect(declared('.feed-table', 'min-width')).toBe('900px');
  });

  it('swaps the feed table for one card per event below 768 px', () => {
    // `display: none` rather than an off-screen position: the hidden half
    // leaves the tab order and the accessibility tree.
    const phone = phoneRules();
    expect(phone).toMatch(/\.feed-scroll \{\s*display: none/);
    expect(phone).toMatch(/\.feed-cards \{\s*display: flex/);
    expect(declared('.feed-cards', 'display')).toBe('none');
  });

  it('gives the phone feed bar 44 px controls and its own chip scroller', () => {
    const phone = phoneRules();
    expect(phone).toMatch(/\.feed-actions \.btn \{[^}]*min-height: 44px/);
    expect(phone).toMatch(/\.feed-chipset \.chip \{[^}]*height: 44px/);
    expect(phone).toMatch(/\.feed-chipset \.chips \{[^}]*overflow-x: auto/);
  });

  it('draws the residual swatch and segment as a texture, not a second hue', () => {
    // The residual is a remainder rather than a structure, so it is the one
    // band that never takes a hue — which is also what frees the hue it would
    // otherwise have spent, in a palette that has none to spare. The swatch and
    // the bar segment share one rule so they cannot drift apart.
    const rule = declared('.seg-residual,\n.sw-residual', 'background-image');
    expect(rule).toContain('repeating-linear-gradient');
    expect(rule).toContain('--memory-residual-line');
  });

  it('leaves the memory rows on the layout grid rather than a private track', () => {
    // Both memory rows are `row c4`. A private `grid-template-columns` on them
    // loses to `.row > * { grid-column: span 12 }` in `layout.css` and renders
    // eight full-width cards stacked — which is exactly what the first attempt
    // did, and what no unit test caught. Anything that reintroduces a private
    // track here is the same bug returning.
    expect(declared('.memory-row-4', 'grid-template-columns')).toBeNull();
    // `.kpi-rail` carries no rule of its own at all — the layout grid is the
    // whole of its geometry.
    expect(CSS).not.toMatch(/\.kpi-rail[^{]*\{[^}]*grid-template-columns/);
  });
});
