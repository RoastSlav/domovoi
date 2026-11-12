import type { ReactNode } from "react";
import { Icon } from "../design/icons";
import type { Tone } from "../design/tone";
import { DataRow } from "./DataRow";
import { InfoTip } from "./InfoTip";
import { Skeleton } from "./Skeleton";
import styles from "./data.module.css";

/** One column of a data table. */
export interface Column<T> {
  cell: (row: T) => ReactNode;
  /** Keeps the header for assistive technology only: for a column of row actions. */
  hideLabel?: boolean;
  /** Explains a header whose meaning is not obvious, behind an info button. */
  hint?: string;
  /** Unique within the table. */
  key: string;
  /** The header text; also the label beside the value when the table is stacked. */
  label: string;
  /**
   * Long content: `clamp2` stops after two lines, `ellipsis` after one, ending in "…". Give
   * `tip` so the full value is still on offer, and use `layout="fixed"` for it to bite.
   */
  truncate?: "clamp2" | "ellipsis";
  /** The full text of a truncated cell, as a tooltip. */
  tip?: (row: T) => string;
  /** A CSS length for the column under `layout="fixed"` (`8rem`); the rest share what is left. */
  width?: string;
  /** Right-aligned tabular digits, for numbers that should line up. */
  numeric?: boolean;
  /** The header becomes a button that asks the page to sort by this column (see `sort`). */
  sortable?: boolean;
}

/** Which column a table is sorted by, and which way. */
export interface TableSort {
  direction: "ascending" | "descending";
  key: string;
}

/**
 * A semantic table in a bordered 8 px surface: a 36 px sticky header in sentence case, rows
 * of `--row-h` with a hairline between them and a raised hover, numeric columns right-aligned
 * in header and cell, an actions column that hugs its content, an optional tone stripe per
 * row, skeleton rows while loading and an empty slot. In a narrow container it becomes a
 * list of stacked cards with each value labelled.
 *
 * A row is one click target: `rowHref` turns the first column's content into a link that
 * covers the row (the link stays the accessible element and the keyboard target), and
 * `onRowClick` forwards a click anywhere on the row, unless it landed on a link, button or
 * other control. With `onRowClick` alone, put a link or button in the first column for the
 * keyboard.
 *
 * Long content is cut per column (`truncate`, with `tip` for the full text), and
 * `layout="fixed"` with a `width` per column keeps several tables aligned.
 *
 * Sorting is the page's: mark columns `sortable`, pass the current `sort` and an `onSort`
 * that receives the clicked column's key, and hand the table rows already in that order.
 * The header then carries `aria-sort` and a button that says what it will do.
 *
 * Rows are memoized on the row object and on `columns`, so a live update that changes one
 * row redraws one row: keep `columns` stable (`useMemo` or a module constant) and give
 * unchanged rows the same object (TanStack Query does this by default).
 *
 * Use it for many things with the same fields. The details of one thing are a
 * `KeyValueList`; a handful of rich items are `Card`s.
 */
export function DataTable<T>({
  caption,
  columns,
  rows,
  rowKey,
  rowTone,
  rowState,
  rowHref,
  loading = false,
  empty,
  sort,
  onSort,
  onRowClick,
  layout = "auto",
}: {
  /**
   * `fixed` makes the column widths come from `width` and not from the content, so tables
   * on one page (one per group) line up column for column.
   */
  layout?: "auto" | "fixed";
  /** Names the table for assistive technology; not shown. */
  caption: string;
  columns: readonly Column<T>[];
  /** Shown in place of the rows when there are none: an `EmptyState`. */
  empty?: ReactNode;
  /** Shows skeleton rows in place of the data. */
  loading?: boolean;
  /** Called with the key of a `sortable` column whose header was clicked. */
  onSort?: (key: string) => void;
  /**
   * Called with the row when its body is clicked (not a control inside it). Keep it stable
   * (`useCallback`): a new function redraws every row.
   */
  onRowClick?: (row: T) => void;
  /** The column the rows are sorted by, so its header can say so. */
  sort?: TableSort;
  /** Where a row leads; the first column's content becomes the link. */
  rowHref?: (row: T) => string;
  rowKey: (row: T) => string;
  /**
   * `urgent` gives the row a very faint danger tint: only for things that need a person
   * now. `quiet` mutes its text: for things that are over.
   */
  rowState?: (row: T) => "urgent" | "quiet" | undefined;
  rowTone?: (row: T) => Tone | undefined;
  rows: readonly T[];
}) {
  return (
    <div className={styles.wrap}>
      <div className={styles.frame}>
        <table className={styles.table} data-layout={layout} aria-busy={loading || undefined}>
          <caption className="visually-hidden">{caption}</caption>
          {layout === "fixed" && (
            <colgroup>
              {columns.map((column) => (
                // The width is the page's own, per column: a computed value, so inline.
                <col key={column.key} style={column.width ? { width: column.width } : undefined} />
              ))}
            </colgroup>
          )}
          <thead>
            <tr>
              {columns.map((column) => {
                const sorted = sort?.key === column.key ? sort.direction : undefined;
                const label = column.hideLabel ? (
                  <span className="visually-hidden">{column.label}</span>
                ) : (
                  column.label
                );
                return (
                  <th
                    key={column.key}
                    scope="col"
                    data-numeric={column.numeric || undefined}
                    data-actions={column.hideLabel || undefined}
                    aria-sort={column.sortable ? (sorted ?? "none") : undefined}
                  >
                    <span className={styles.headCell}>
                      {column.sortable && onSort ? (
                        <button
                          type="button"
                          className={styles.sortButton}
                          data-sorted={sorted}
                          onClick={() => onSort(column.key)}
                        >
                          {label}
                          <Icon
                            name={sorted === "descending" ? "chevron-down" : "chevron-up"}
                            size={14}
                          />
                        </button>
                      ) : (
                        label
                      )}
                      {column.hint && <InfoTip title={column.label}>{column.hint}</InfoTip>}
                    </span>
                  </th>
                );
              })}
            </tr>
          </thead>
          <tbody>
            {loading && (
              <tr>
                <td colSpan={columns.length} className={styles.emptyCell}>
                  <Skeleton shape="rows" />
                </td>
              </tr>
            )}
            {!loading && rows.length === 0 && empty && (
              <tr>
                <td colSpan={columns.length} className={styles.emptyCell}>
                  {empty}
                </td>
              </tr>
            )}
            {!loading &&
              rows.map((row) => (
                <DataRow
                  key={rowKey(row)}
                  row={row}
                  columns={columns}
                  state={rowState?.(row)}
                  tone={rowTone?.(row)}
                  href={rowHref?.(row)}
                  onClick={onRowClick}
                />
              ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
