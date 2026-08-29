import type { RefObject } from 'preact';
import { useState } from 'preact/hooks';

/**
 * A `<textarea>` with a synchronised gutter and per-line error anchors. **No
 * editor dependency is added** — the whole of it is one absolutely positioned
 * gutter, two overlay layers and the scroll handler that keeps the three
 * aligned. A code-editor package would be 100 KB+ against a 150 KB budget for
 * a document of a few dozen lines.
 *
 * The line height is fixed and set from one constant, in JS rather than only
 * in CSS: the overlays are positioned arithmetically, so a stylesheet and a
 * script disagreeing about it would silently anchor every band one line off.
 */

export const EDITOR_LINE_HEIGHT = 22;

/**
 * Where line `lineNumber` sits inside the editor box, given the textarea's
 * current scroll offset. One-based, and it may legitimately be negative — a
 * band for a line scrolled off the top is clipped by the overlay rather than
 * moved.
 */
export function lineTop(
  lineNumber: number,
  lineHeightPx: number,
  scrollTopPx: number,
): number {
  return (lineNumber - 1) * lineHeightPx - scrollTopPx;
}

/**
 * The character range of one-based line `lineNumber` in `text`, clamped to the
 * text. This is what "anchors to its line" functionally requires: selecting
 * the range puts the caret on the offending rule at any width, including the
 * 390 px one where a floating callout has nowhere to go.
 */
export function lineRange(
  text: string,
  lineNumber: number,
): { start: number; end: number } {
  const lines = text.split('\n');
  const index = Math.min(Math.max(lineNumber, 1), lines.length) - 1;
  let start = 0;
  for (let cursor = 0; cursor < index; cursor += 1) {
    start += (lines[cursor] ?? '').length + 1;
  }
  return { start, end: start + (lines[index] ?? '').length };
}

export interface EditorAnchor {
  line: number;
  message: string;
}

export function LineEditor({
  value,
  onInput,
  anchors,
  disabled,
  textareaRef,
  label,
}: {
  value: string;
  onInput: (next: string) => void;
  /** One per reported bad line. Empty while nothing has been rejected. */
  anchors: readonly EditorAnchor[];
  disabled: boolean;
  textareaRef: RefObject<HTMLTextAreaElement>;
  label: string;
}) {
  const [scrollTop, setScrollTop] = useState(0);
  const lines = value.split('\n');
  // The box is a whole number of lines so the gutter, the bands and the text
  // cannot end a fraction of a line apart. Fourteen is the artboard's document
  // height; past twenty-six the textarea scrolls inside itself and the two
  // overlays follow it.
  const height =
    Math.min(Math.max(lines.length, 14), 26) * EDITOR_LINE_HEIGHT;

  return (
    <div class="editor" style={{ height: `${String(height)}px` }}>
      <div class="editor-gutter" aria-hidden="true">
        <div
          class="editor-gutter-inner"
          style={{ top: `${String(lineTop(1, EDITOR_LINE_HEIGHT, scrollTop))}px` }}
        >
          {lines.map((_, index) => (
            <span key={index} class="mono">
              {index + 1}
            </span>
          ))}
        </div>
      </div>

      {/* Behind the textarea, which is transparent — a row tint that does not
          hide the text sitting on it. */}
      <div class="editor-bands" aria-hidden="true">
        {anchors.map((anchor) => (
          <span
            key={`band-${String(anchor.line)}`}
            class="editor-band"
            style={{
              top: `${String(lineTop(anchor.line, EDITOR_LINE_HEIGHT, scrollTop))}px`,
            }}
          />
        ))}
      </div>

      <textarea
        ref={textareaRef}
        class="editor-area mono"
        aria-label={label}
        spellcheck={false}
        autocapitalize="off"
        autocorrect="off"
        disabled={disabled}
        value={value}
        onInput={(event) => onInput(event.currentTarget.value)}
        onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
      />

      {/* Above the textarea and inert, so the callout never eats a click that
          was meant for the text under it. The message is the API's own words
          and nothing more: the `422` envelope carries no diagnosis, and
          inventing one would put text on screen no field backs.

          It sits on the bad line's **own** row, in the empty space after that
          line's text — the row is monospace at the textarea's size, so one
          `ch` is exactly one column and the offset needs no measurement. It
          used to float over the row beneath, which hid that line's text
          outright: measured at 1400 px and at 390 px, the line under a callout
          could not be read at all. Clamped at 60 % so a long bad line leaves
          the message somewhere to go; it then overlaps the tail of the line it
          is about, never a different one. */}
      <div class="editor-callouts" aria-hidden="true">
        {anchors.map((anchor) => (
          <span
            key={`callout-${String(anchor.line)}`}
            class="editor-callout-row mono"
            style={{
              top: `${String(lineTop(anchor.line, EDITOR_LINE_HEIGHT, scrollTop))}px`,
              paddingLeft: `min(calc(4px + ${String((lines[anchor.line - 1] ?? '').length + 1)}ch), 60%)`,
            }}
          >
            <span class="editor-callout">
              line {anchor.line} — {anchor.message}
            </span>
          </span>
        ))}
      </div>
    </div>
  );
}
