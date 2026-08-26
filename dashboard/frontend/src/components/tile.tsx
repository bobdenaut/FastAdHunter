import { Link } from '../router/link';
import { Icon } from '../shell/icon';

/** Accent roles are fixed and never reused for another meaning
 *  (visual-system.md §Tiles). */
export type TileAccent = 'volume' | 'blocked' | 'ratio' | 'healthy' | 'zero';

export function Tile({
  label,
  figure,
  accent,
  glyph,
  footer,
  href,
}: {
  label: string;
  figure: string;
  accent: TileAccent;
  glyph: string;
  footer?: string;
  href?: string;
}) {
  return (
    <div class={`tile ${accent}`}>
      <Icon name={glyph} size={64} className="ic" />
      <div class="in">
        <div class="lb">{label}</div>
        <div class="n">{figure}</div>
      </div>
      {footer !== undefined &&
        (href === undefined ? (
          <div class="ft">{footer}</div>
        ) : (
          <Link href={href} class="ft">
            {footer}
            <Icon name="arrow-right" size={13} />
          </Link>
        ))}
    </div>
  );
}
