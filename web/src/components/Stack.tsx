import type { ReactNode } from "react";
import styles from "./layout.module.css";

/** A step on the 4 px spacing grid: `--space-1` … `--space-6`. */
export type Gap = 1 | 2 | 3 | 4 | 5 | 6;

/**
 * Children in a column with an even gap between them.
 *
 * Use it for the vertical rhythm of a page, a card or a form. Do not add margins to the
 * children to space them.
 */
export function Stack({ gap = 4, children }: { children: ReactNode; gap?: Gap }) {
  return (
    <div className={styles.stack} data-gap={gap}>
      {children}
    </div>
  );
}

/**
 * Children in a row with an even gap, wrapping onto more lines when they do not fit.
 *
 * Use it for toolbars, button rows and rows of pills. `justify="between"` pushes the two
 * ends apart.
 */
export function Cluster({
  gap = 2,
  justify = "start",
  align = "center",
  wrap = true,
  children,
}: {
  align?: "start" | "center" | "baseline";
  children: ReactNode;
  gap?: Gap;
  justify?: "start" | "between" | "end";
  wrap?: boolean;
}) {
  return (
    <div
      className={styles.cluster}
      data-gap={gap}
      data-justify={justify}
      data-align={align}
      data-wrap={wrap || undefined}
    >
      {children}
    </div>
  );
}

/**
 * Children in as many equal columns as fit, each at least `min` wide (`xs` 8 rem, `sm` 12
 * rem, `md` 18 rem, `lg` 26 rem); one column on a phone, except `xs`, which keeps two. With
 * `fill` the columns share the whole row even when there are fewer children than columns;
 * without it empty columns stay empty, so one card is never stretched across the page.
 *
 * Use it for cards of the same kind. For a table of values use `DataTable`.
 */
export function Grid({
  min = "md",
  gap = 4,
  fill = false,
  children,
}: {
  children: ReactNode;
  /** Stretch the children over the row (`auto-fit`): for a fixed few, like dashboard tiles. */
  fill?: boolean;
  gap?: Gap;
  min?: "xs" | "sm" | "md" | "lg";
}) {
  return (
    <div className={styles.grid} data-min={min} data-gap={gap} data-fill={fill || undefined}>
      {children}
    </div>
  );
}
