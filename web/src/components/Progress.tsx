import { useId } from "react";
import styles from "./state.module.css";

/**
 * A moving bar for something long that is running, with a line saying what, placed where
 * the result will appear ("Triage is reading the logs…").
 *
 * Use it for work that takes seconds. For a moment's wait on a button, use the button's
 * `loading`.
 */
export function Progress({ label }: { label: string }) {
  const id = useId();
  return (
    <div role="status">
      <p className={styles.progressLabel} id={id}>
        {label}
      </p>
      <div className={styles.progress} role="progressbar" aria-labelledby={id} />
    </div>
  );
}
