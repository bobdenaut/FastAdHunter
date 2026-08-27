import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

/**
 * The donut's five slice colours, read out of `tokens.css` and measured.
 *
 * The defect this pins shipped once and was found by looking at the page rather
 * than by any test: `--series-4` and `--series-5` were both neutral greys, so
 * they had nothing but luminance to separate them, and it was not enough in
 * either theme — 1.87 : 1 light, 2.02 : 1 dark. The fifth slice additionally
 * failed to clear the card it is drawn on (1.62 : 1 against white, 2.82 : 1
 * against the dark surface), so a 2 % arc simply was not there.
 *
 * Two greys cannot be 3 : 1 apart *and* both 3 : 1 above a white card — the
 * neutral ramp is not that long. So the rule the palette actually follows is
 * the one asserted here: **hue carries the category, luminance carries
 * visibility.** Series 1–3 sit 1.16–1.46 apart in contrast and are told apart
 * by hue alone; series 5 now does the same, and every slice that has to be
 * findable on its own clears its background.
 *
 * jsdom computes no colour, so the sheet is read as text. Contrast is WCAG 2.x
 * relative luminance, the same formula the browser measurements used.
 */

const CSS = readFileSync(
  fileURLToPath(new URL('./tokens.css', import.meta.url)),
  'utf8',
);

/** `/* … *\/` removed so a commented-out value cannot satisfy a check. */
function withoutComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, '');
}

/** Every value of one custom property, in source order: light, then the two
 *  dark blocks. */
function values(property: string): string[] {
  const matches = withoutComments(CSS).matchAll(
    new RegExp(`(?:^|[;{\\s])${property}\\s*:\\s*(#[0-9a-fA-F]{3,8})\\s*;`, 'g'),
  );
  return [...matches].map((hit) => hit[1] ?? '');
}

function channels(hex: string): number[] {
  const raw = hex.replace('#', '');
  const full =
    raw.length === 3
      ? raw
          .split('')
          .map((char) => char + char)
          .join('')
      : raw;
  return [0, 2, 4].map((at) => Number.parseInt(full.slice(at, at + 2), 16));
}

function luminance(hex: string): number {
  const linear = channels(hex).map((value) => {
    const unit = value / 255;
    return unit <= 0.03928
      ? unit / 12.92
      : Math.pow((unit + 0.055) / 1.055, 2.4);
  });
  return (
    0.2126 * (linear[0] ?? 0) +
    0.7152 * (linear[1] ?? 0) +
    0.0722 * (linear[2] ?? 0)
  );
}

function contrast(a: string, b: string): number {
  const first = luminance(a);
  const second = luminance(b);
  const [high, low] = first > second ? [first, second] : [second, first];
  return (high + 0.05) / (low + 0.05);
}

/** How far a colour is from neutral: 0 is a pure grey. */
function chroma(hex: string): number {
  const parts = channels(hex);
  return Math.max(...parts) - Math.min(...parts);
}

const SERIES = [1, 2, 3, 4, 5] as const;

/** Light, `prefers-color-scheme: dark`, `[data-theme='dark']` — the three
 *  blocks every token in this sheet is defined in. */
const PALETTES = 3;

describe('the donut series palette', () => {
  it('defines all five series in all three palette blocks', () => {
    for (const index of SERIES) {
      expect(
        values(`--series-${String(index)}`),
        `--series-${String(index)}`,
      ).toHaveLength(PALETTES);
    }
    expect(values('--surface')).toHaveLength(PALETTES);
    expect(values('--track')).toHaveLength(PALETTES);
  });

  it('keeps the fifth slice visible on its own card and track', () => {
    // The one that was not: 1.62 : 1 on white, 2.82 : 1 on the dark surface.
    const fifth = values('--series-5');
    const surfaces = values('--surface');
    const tracks = values('--track');
    fifth.forEach((colour, palette) => {
      expect(
        contrast(colour, surfaces[palette] ?? ''),
        `--series-5 on --surface, palette ${String(palette)}`,
      ).toBeGreaterThanOrEqual(3);
      expect(
        contrast(colour, tracks[palette] ?? ''),
        `--series-5 on --track, palette ${String(palette)}`,
      ).toBeGreaterThanOrEqual(3);
    });
  });

  it('keeps the fourth slice above its own card', () => {
    // 3.04 : 1 in the light theme — thin, and inherited from `Main.dc.html`.
    // A darker card or a lighter grey breaks it, which is the point.
    const fourth = values('--series-4');
    const surfaces = values('--surface');
    fourth.forEach((colour, palette) => {
      expect(
        contrast(colour, surfaces[palette] ?? ''),
        `--series-4 on --surface, palette ${String(palette)}`,
      ).toBeGreaterThanOrEqual(3);
    });
  });

  it('does not draw two neutral slices side by side', () => {
    // The actual defect. Four and five are adjacent in the ring and in the
    // legend, so they are the one pair a reader compares directly; with both
    // neutral, luminance was the only separator and it was under 2 : 1 in both
    // themes. Five is chromatic now, four stays the grey the artboard draws.
    const fourth = values('--series-4');
    const fifth = values('--series-5');
    fourth.forEach((colour, palette) => {
      expect(chroma(colour), `--series-4 is a grey, palette ${String(palette)}`)
        .toBeLessThan(32);
    });
    fifth.forEach((colour, palette) => {
      expect(
        chroma(colour),
        `--series-5 is not a grey, palette ${String(palette)}`,
      ).toBeGreaterThan(48);
    });
  });
});
