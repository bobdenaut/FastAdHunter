import type { ComponentChildren } from 'preact';

export interface TopRow {
  key: string;
  /** The name given the space, ellipsised on overflow. */
  primary: ComponentChildren;
  /** Shown beside `primary` on desktop and hidden on a phone, where the row has
   *  no room for both. */
  secondary?: ComponentChildren;
  count: number;
  /** A second right-aligned figure — the Blocked column both top-client
   *  artboards draw differently at the two widths. */
  extra?: ComponentChildren;
  /** 0..1 of the widest row rendered. */
  share: number;
  /** Tints the bar for a blocked table. */
  tone?: 'blocked';
}

/**
 * One DOM for both widths. Desktop is a table with a Frequency column; the
 * phone artboard is a row with the name given the space, the bar next and the
 * count right-aligned. Those are the same three cells in a different order and
 * at different sizes, so the order is a CSS `order` and the visible row count a
 * CSS rule — not a viewport listener and not a second component.
 */
export function TopList({
  columns,
  frequencyLabel = 'Frequency',
  rows,
  more,
  expanded = false,
  variant,
}: {
  columns: [string, string, string?];
  /** The bar column's heading. `Main.dc.html` heads it `Frequency` on the two
   *  domain tables and `Share` on Top clients. */
  frequencyLabel?: string;
  rows: readonly TopRow[];
  /** The trailing 44 px row the phone artboards draw. Desktop hides it. */
  more?: ComponentChildren;
  expanded?: boolean;
  /** `clients` drops the frequency bar below 768 px: the phone artboard draws
   *  name, queries and blocked percentage, and a bar as well does not fit. */
  variant?: 'clients';
}) {
  const classes = ['toplist'];
  if (expanded) classes.push('is-expanded');
  if (variant !== undefined) classes.push(`toplist-${variant}`);

  return (
    <div class={classes.join(' ')}>
      <div class="toplist-head" aria-hidden="true">
        <span class="d">{columns[0]}</span>
        <span class="n">{columns[1]}</span>
        {columns[2] !== undefined && <span class="x">{columns[2]}</span>}
        <span class="f">{frequencyLabel}</span>
      </div>
      <div class="toplist-rows">
        {rows.map((row) => (
          <div class="toplist-row" key={row.key}>
            <span class="d">
              {row.primary}
              {row.secondary !== undefined && (
                <span class="d-sec">{row.secondary}</span>
              )}
            </span>
            <span class="n">{row.count.toLocaleString()}</span>
            {row.extra !== undefined && <span class="x">{row.extra}</span>}
            <span class="f">
              <div class="bar">
                <span
                  class={row.tone === 'blocked' ? 'blocked' : undefined}
                  style={{ width: `${String(Math.max(0, Math.min(1, row.share)) * 100)}%` }}
                />
              </div>
            </span>
          </div>
        ))}
      </div>
      {more !== undefined && <div class="toplist-more">{more}</div>}
    </div>
  );
}
