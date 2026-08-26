import spriteUrl from '../assets/sprite.svg';

/**
 * One self-hosted SVG sprite of the glyphs actually used. It is imported as a
 * URL so Vite emits it hashed under `/assets/`, which p5-01's handler serves
 * `immutable`; inlining it as a data URI would put it inside the JS chunk and
 * invalidate on every code edit.
 */
export function Icon({
  name,
  size = 15,
  className,
}: {
  name: string;
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      class={className}
      aria-hidden="true"
      focusable="false"
    >
      <use href={`${spriteUrl}#${name}`} />
    </svg>
  );
}
