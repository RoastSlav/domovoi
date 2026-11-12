import type { ReactNode } from "react";
import { Link } from "react-router";
import type { Tone } from "../design/tone";
import styles from "./stat.module.css";

/**
 * One number of a dashboard: a small label (with a tone dot when the number means
 * something), the value in large tabular digits and an optional line under it. With `to` the
 * whole tile is a link card.
 *
 * Use it in a row (a `Grid` of `sm` columns) to answer "is anything wrong?" at a glance. The
 * dot is colour alone, so put what it means in `hint` ("2 hosts offline"). Do not use it for
 * a status (that is a `StatusPill`) or for a number inside a table. In a narrow container
 * (under 14 rem) the tile goes compact: less padding, a 20 px value, a one-line hint.
 */
export function StatTile({
  label,
  value,
  tone,
  hint,
  to,
}: {
  /** A line under the value: a delta, or what the dot means. */
  hint?: ReactNode;
  /** What is counted: "Open incidents". */
  label: string;
  /** Draws a dot in this tone before the label. */
  tone?: Tone | undefined;
  /** Where the tile leads; it then renders as a link. */
  to?: string;
  /** The number, or a short phrase such as "3 of 4". */
  value: ReactNode;
}) {
  const content = (
    <div className={styles.inner}>
      <p className={styles.label}>
        {tone && <span className={styles.dot} data-tone={tone} aria-hidden="true" />}
        {label}
      </p>
      <p className={styles.value}>{value}</p>
      {hint && <p className={styles.hint}>{hint}</p>}
    </div>
  );
  return to ? (
    <Link to={to} className={styles.tile} data-interactive>
      {content}
    </Link>
  ) : (
    <div className={styles.tile}>{content}</div>
  );
}
