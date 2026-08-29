import { describe, expect, it } from 'vitest';
import { EDITOR_LINE_HEIGHT, lineRange, lineTop } from './line-editor';

describe('lineTop', () => {
  it('places line 1 at the top of an unscrolled editor', () => {
    expect(lineTop(1, EDITOR_LINE_HEIGHT, 0)).toBe(0);
  });

  it('advances one line height per line', () => {
    expect(lineTop(2, 22, 0)).toBe(22);
    expect(lineTop(8, 22, 0)).toBe(154);
  });

  it('subtracts the textarea scroll offset, so the three layers stay aligned', () => {
    expect(lineTop(8, 22, 44)).toBe(110);
    expect(lineTop(1, 22, 44)).toBe(-44);
  });

  // A band for a line scrolled off the top is clipped by the overlay rather
  // than clamped: clamping would stack every off-screen band on line one.
  it('returns a negative offset rather than clamping', () => {
    expect(lineTop(3, 22, 200)).toBeLessThan(0);
  });
});

describe('lineRange', () => {
  const text = 'alpha\nbeta\n\ndelta';

  it('selects the whole of a one-based line', () => {
    expect(lineRange(text, 1)).toEqual({ start: 0, end: 5 });
    expect(lineRange(text, 2)).toEqual({ start: 6, end: 10 });
    expect(lineRange(text, 4)).toEqual({ start: 12, end: 17 });
  });

  it('collapses to a caret on an empty line', () => {
    expect(lineRange(text, 3)).toEqual({ start: 11, end: 11 });
  });

  it('clamps a line number the document does not have', () => {
    expect(lineRange(text, 99)).toEqual({ start: 12, end: 17 });
    expect(lineRange(text, 0)).toEqual({ start: 0, end: 5 });
  });

  it('counts the separator, so CRLF-free text round-trips exactly', () => {
    const document = 'a\nbb\nccc';
    expect(document.slice(lineRange(document, 2).start, lineRange(document, 2).end)).toBe('bb');
    expect(document.slice(lineRange(document, 3).start, lineRange(document, 3).end)).toBe('ccc');
  });
});
