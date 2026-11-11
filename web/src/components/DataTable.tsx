import type { ReactNode } from "react";
import { Link } from "react-router";
import { Icon } from "../design/icons";
import type { Tone } from "../design/tone";
import { InfoTip } from "./InfoTip";
import { Skeleton } from "./Skeleton";
import { toneStripe } from "./ToneStripe";
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
 * A semantic table: sticky header, an optional tone stripe per row, an optional link that
 * makes the whole row clickable (the link stays the accessible element, in the first
 * column), skeleton rows while loading and an empty slot. In a narrow container it becomes
 * a list of stacked cards with each value labelled.
 *
 * Sorting is the page's: mark columns `sortable`, pass the current `sort` and an `onSort`
 * that receives the clicked column's key, and hand the table rows already in that order.
 * The header then carries `aria-sort` and a button that says what it will do.
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
}: {
  /** Names the table for assistive technology; not shown. */
  caption: string;
  columns: readonly Column<T>[];
  /** Shown in place of the rows when there are none: an `EmptyState`. */
  empty?: ReactNode;
  /** Shows skeleton rows in place of the data. */
  loading?: boolean;
  /** Called with the key of a `sortable` column whose header was clicked. */
  onSort?: (key: string) => void;
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
      <table className={styles.table} aria-busy={loading || undefined}>
        <caption className="visually-hidden">{caption}</caption>
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
              <tr key={rowKey(row)} data-state={rowState?.(row)} {...toneStripe(rowTone?.(row))}>
                {columns.map((column, index) => (
                  <td
                    key={column.key}
                    data-label={column.hideLabel ? undefined : column.label}
                    data-numeric={column.numeric || undefined}
                    data-primary={index === 0 || undefined}
                  >
                    {index === 0 && rowHref ? (
                      <Link to={rowHref(row)} className={styles.rowLink}>
                        {column.cell(row)}
                      </Link>
                    ) : (
                      column.cell(row)
                    )}
                  </td>
                ))}
              </tr>
            ))}
        </tbody>
      </table>
    </div>
  );
}
