import { memo, type MouseEvent, type ReactNode } from "react";
import { Link } from "react-router";
import type { Tone } from "../design/tone";
import type { Column } from "./DataTable";
import { toneStripe } from "./ToneStripe";
import { Tooltip } from "./Tooltip";
import styles from "./data.module.css";

/** What a click that should not count as a row click landed on: a control of its own. */
const CONTROL =
  'a, button, input, select, textarea, label, summary, [role="button"], [role^="menuitem"]';

interface DataRowProps<T> {
  columns: readonly Column<T>[];
  href: string | undefined;
  onClick: ((row: T) => void) | undefined;
  row: T;
  state: "urgent" | "quiet" | undefined;
  tone: Tone | undefined;
}

/** A cell's content, cut to the column's `truncate` with its full text as a tooltip. */
function content<T>(column: Column<T>, row: T): ReactNode {
  const cell = column.cell(row);
  if (!column.truncate) {
    return cell;
  }
  const cut = (
    <span className={column.truncate === "clamp2" ? styles.clamp2 : styles.ellipsis}>{cell}</span>
  );
  const tip = column.tip?.(row);
  return tip ? <Tooltip content={tip}>{cut}</Tooltip> : cut;
}

/**
 * One body row of a `DataTable`. Memoized: it redraws only when its row, the columns, its
 * look or its click handler change. A click forwards to `onClick` unless it landed on a
 * control inside the row, or was already handled.
 */
export const DataRow = memo(function DataRow<T>({
  row,
  columns,
  state,
  tone,
  href,
  onClick,
}: DataRowProps<T>) {
  const forward = onClick
    ? (event: MouseEvent<HTMLTableRowElement>): void => {
        const target = event.target instanceof Element ? event.target : null;
        if (!event.defaultPrevented && !target?.closest(CONTROL)) {
          onClick(row);
        }
      }
    : undefined;
  return (
    <tr
      data-state={state}
      data-clickable={href || onClick ? true : undefined}
      onClick={forward}
      {...toneStripe(tone)}
    >
      {columns.map((column, index) => (
        <td
          key={column.key}
          data-label={column.hideLabel ? undefined : column.label}
          data-numeric={column.numeric || undefined}
          data-actions={column.hideLabel || undefined}
          data-primary={index === 0 || undefined}
          data-truncate={column.truncate}
        >
          {index === 0 && href ? (
            <Link to={href} className={styles.rowLink}>
              {content(column, row)}
            </Link>
          ) : (
            content(column, row)
          )}
        </td>
      ))}
    </tr>
  );
}) as <T>(props: DataRowProps<T>) => ReactNode;
