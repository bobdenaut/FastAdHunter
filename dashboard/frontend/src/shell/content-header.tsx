import type { ComponentChildren } from 'preact';
import { Icon } from './icon';

/**
 * Page name plus a short line of context. `cluster` is the refresh cluster a
 * route reading exactly one polled endpoint puts here, beside the title — the
 * placement `Cache` draws.
 *
 * `actions` is the page's own header buttons — `Discard` / `Validate and save`,
 * `New policy`. Three of `p5-07`'s four artboards draw them there, and the
 * shell renders a title alone, which is why Lists' pair ended up in the page
 * body. A route declaring `ownsHeader` renders this component itself and fills
 * both slots; the shell then renders no header of its own.
 */
export function ContentHeader({
  title,
  context,
  cluster,
  actions,
  glyph,
}: {
  title: string;
  context?: ComponentChildren;
  cluster?: ComponentChildren;
  actions?: ComponentChildren;
  /** A sprite name. Draws the artboard's icon tile beside the title — the
   *  treatment `Diagnostics · Memory` uses. Omitted, the header is unchanged. */
  glyph?: string;
}) {
  // The title block is wrapped **only** when a glyph asks for the row. Twelve
  // pages pass no glyph and their markup stays byte-identical to what it was
  // before this slot existed, which is what `content-header.test.tsx` pins —
  // a new slot must not reflow the pages that did not ask for it.
  const lead = (
    <div>
      <p class="h1">{title}</p>
      {context !== undefined && <p class="sub">{context}</p>}
    </div>
  );

  return (
    <div class="hd">
      {glyph === undefined ? (
        lead
      ) : (
        <div class="hd-lead">
          <span class="hd-glyph">
            <Icon name={glyph} size={21} />
          </span>
          {lead}
        </div>
      )}
      {cluster}
      {actions !== undefined && <div class="hd-actions">{actions}</div>}
    </div>
  );
}
