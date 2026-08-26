import type { ComponentChildren } from 'preact';

export interface Column<Row> {
  key: string;
  header: string;
  /** Right-aligned and tabular — figures line up column-wise. */
  numeric?: boolean;
  width?: string;
  cell: (row: Row) => ComponentChildren;
}

/**
 * Figures right-aligned and tabular, zebra off, hover on, and the whole table
 * scrolling inside its own container so the page body never scrolls sideways.
 */
export function Table<Row>({
  columns,
  rows,
  rowKey,
  empty,
}: {
  columns: ReadonlyArray<Column<Row>>;
  rows: readonly Row[];
  rowKey: (row: Row, index: number) => string;
  empty?: ComponentChildren;
}) {
  if (rows.length === 0 && empty !== undefined) return <>{empty}</>;
  return (
    <div class="table-scroll">
      <table class="t">
        <thead>
          <tr>
            {columns.map((column) => (
              <th
                key={column.key}
                class={column.numeric === true ? 'num' : undefined}
                style={column.width === undefined ? undefined : { width: column.width }}
              >
                {column.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr key={rowKey(row, index)}>
              {columns.map((column) => (
                <td
                  key={column.key}
                  class={column.numeric === true ? 'num' : undefined}
                >
                  {column.cell(row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** The translucent frequency bar behind a count in a top-N table. `share` is
 *  clamped, so a figure larger than the maximum cannot overflow the cell. */
export function FrequencyBar({ share }: { share: number }) {
  const width = Math.max(0, Math.min(1, share)) * 100;
  return (
    <div class="bar">
      <span style={{ width: `${width}%` }} />
    </div>
  );
}
