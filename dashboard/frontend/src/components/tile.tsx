import { Link } from '../router/link';
import { Icon } from '../shell/icon';

/** Accent roles are fixed and never reused for another meaning
 *  (visual-system.md §Tiles). */
export type TileAccent = 'volume' | 'blocked' | 'ratio' | 'healthy' | 'zero';

export function Tile({
  label,
  labelShort,
  figure,
  accent,
  glyph,
  footer,
  footerShort,
  href,
}: {
  label: string;
  /**
   * The phone artboard's shortened label (`Blocked`, `Cache hit`). Same
   * mechanism as `footerShort`: both are rendered and one is hidden per
   * breakpoint, because a JS width branch would put a listener on every tile.
   */
  labelShort?: string;
  figure: string;
  accent: TileAccent;
  glyph: string;
  footer?: string;
  /**
   * The phone artboard's shortened footer ("6 clients", "proxy", "3 refused").
   * Both are rendered and one is hidden per breakpoint: the swap is a CSS
   * question, and a JS width branch would put a listener on every tile.
   */
  footerShort?: string;
  href?: string;
}) {
  const strip =
    footerShort === undefined ? (
      footer
    ) : (
      <>
        <span class="ft-long">{footer}</span>
        <span class="ft-short">{footerShort}</span>
      </>
    );

  return (
    <div class={`tile ${accent}`}>
      <Icon name={glyph} size={64} className="ic" />
      <div class="in">
        <div class="lb">
          {labelShort === undefined ? (
            label
          ) : (
            <>
              <span class="lb-long">{label}</span>
              <span class="lb-short">{labelShort}</span>
            </>
          )}
        </div>
        <div class="n">{figure}</div>
      </div>
      {footer !== undefined &&
        (href === undefined ? (
          <div class="ft">{strip}</div>
        ) : (
          <Link href={href} class="ft">
            {strip}
            <Icon name="arrow-right" size={13} />
          </Link>
        ))}
    </div>
  );
}
