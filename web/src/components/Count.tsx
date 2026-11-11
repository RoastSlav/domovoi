import type { Tone } from "../design/tone";
import styles from "./status.module.css";

/**
 * A filled number for navigation items, tabs and filter chips. `label` says what is counted
 * ("waiting"), so assistive technology reads "3 waiting" where the eye sees "3".
 *
 * Do not use it for a status or for a number inside a sentence.
 */
export function Count({ tone, value, label }: { label: string; tone: Tone; value: number }) {
  return (
    <span className={styles.count} data-tone={tone}>
      <span aria-hidden="true">{value}</span>
      <span className="visually-hidden">
        {value} {label}
      </span>
    </span>
  );
}
