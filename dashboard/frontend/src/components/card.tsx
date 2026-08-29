import type { ComponentChildren } from 'preact';

/**
 * White in light, raised surface in dark. `secondary` is the title bar's
 * secondary-text slot — where an aggregate a bar chart structurally cannot show
 * lives, and where the refresh cluster follows it in the same right-hand group.
 */
export function Card({
  title,
  secondary,
  tools,
  children,
  bodyClass,
  className,
}: {
  title?: ComponentChildren;
  secondary?: ComponentChildren;
  tools?: ComponentChildren;
  children?: ComponentChildren;
  bodyClass?: string | undefined;
  /** A placement hook. The phone layout reorders and drops whole cards, and
   *  doing that in CSS needs something to select. */
  className?: string | undefined;
}) {
  return (
    <section class={className === undefined ? 'card' : `card ${className}`}>
      {title !== undefined && (
        <h3 class="ch">
          <span>{title}</span>
          {(secondary !== undefined || tools !== undefined) && (
            <span class="ch-right">
              {secondary !== undefined && (
                <span class="note mono">{secondary}</span>
              )}
              {tools}
            </span>
          )}
        </h3>
      )}
      <div class={bodyClass === undefined ? 'bd' : `bd ${bodyClass}`}>
        {children}
      </div>
    </section>
  );
}
