import type { ComponentChildren } from 'preact';

export interface CounterRow {
  label: string;
  value: ComponentChildren;
  /** The artboards print `hits` and `completed` in the good tone — the one
   *  figure per table that is the point of the table. */
  tone?: 'good' | 'bad';
}

/**
 * The label-and-figure list four of this page's cards draw. Headerless on
 * purpose: `Table` carries a header row, and a two-column list of named
 * counters has nothing to head.
 *
 * It lives here rather than inside one of the cards because four of them need
 * it and a sibling card importing another card's export couples two cards that
 * have nothing to do with each other.
 */
export function CounterTable({ rows }: { rows: readonly CounterRow[] }) {
  return (
    <table class="t counters">
      <tbody>
        {rows.map((row) => (
          <tr key={row.label}>
            <td>{row.label}</td>
            <td class={row.tone === undefined ? 'num' : `num ${row.tone}`}>
              {row.value}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
