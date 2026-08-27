import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

/**
 * `tokens.css`' own header states the rule: "no rule outside this file may name
 * a literal colour". Nothing enforced it, so p5-06 removed two literals in one
 * pass and introduced a third in the same edit without anyone noticing.
 *
 * This is the enforcement, and it is an **allowlist rather than a ban** on
 * purpose: the seven sites below predate this task and are not its to move, but
 * they must stay exactly seven. A new literal fails here, so widening the
 * exception becomes a deliberate edit to this list with a reason beside it —
 * which is the whole point.
 */

const SHEETS = ['base.css', 'components.css', 'layout.css'] as const;

/** Hex, `rgb()`/`rgba()`, `hsl()`/`hsla()` — every form the sheets use. */
const LITERAL = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\s*\(/g;

/** A commented-out colour is not a rule, and `tokens.css`' prose names several. */
function withoutComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, '');
}

function read(sheet: string): string {
  return readFileSync(
    fileURLToPath(new URL(`./${sheet}`, import.meta.url)),
    'utf8',
  );
}

function literalsIn(sheet: string): string[] {
  return [...withoutComments(read(sheet)).matchAll(LITERAL)].map((hit) =>
    hit[0].replace(/\s+\($/, '('),
  );
}

/**
 * Inherited from `p5-05`, every one of them a white or a black over a colour
 * the palette already owns:
 *
 * | site | literal |
 * | ---- | ------- |
 * | `.tile`, its glyph, its footer strip | `#fff` × 3 |
 * | `.tile .ft` and its hover | `rgba(` × 2 |
 * | `.btn`, `.chip.on` | `#fff` × 2 |
 * | `.drawer` shadow (`layout.css`) | `rgba(` × 1 |
 */
const INHERITED: Record<string, string[]> = {
  'base.css': [],
  'components.css': [
    '#fff',
    '#fff',
    'rgba(',
    '#fff',
    'rgba(',
    '#fff',
    '#fff',
  ],
  'layout.css': ['rgba('],
};

describe('literal colours outside tokens.css', () => {
  for (const sheet of SHEETS) {
    it(`${sheet} names exactly the inherited ones`, () => {
      expect(literalsIn(sheet)).toEqual(INHERITED[sheet]);
    });
  }

  it('covers the two p5-06 introduced and then tokenised', () => {
    // The tooltip's shadow and the switch knob. Both are `var(--…)` now, and
    // both palettes carry the token — a token defined in one block only is the
    // other half of the same mistake.
    const tokens = withoutComments(
      readFileSync(
        fileURLToPath(new URL('./tokens.css', import.meta.url)),
        'utf8',
      ),
    );
    for (const token of ['--tip-shadow', '--switch-knob']) {
      expect(
        tokens.split(`${token}:`).length - 1,
        `${token} must be defined in all three palette blocks`,
      ).toBe(3);
    }
    const components = withoutComments(
      readFileSync(
        fileURLToPath(new URL('./components.css', import.meta.url)),
        'utf8',
      ),
    );
    expect(components).toContain('box-shadow: 0 4px 14px var(--tip-shadow)');
    expect(components).toContain('background: var(--switch-knob)');
  });
});
