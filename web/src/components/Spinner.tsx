import { Icon } from "../design/icons";
import styles from "./state.module.css";

/**
 * A small turning mark for "this is running", inline with text or inside a button. Give
 * `label` when no words beside it say what is running.
 *
 * Use it for a short wait next to its cause. Do not use it for a page or panel that is
 * loading: that is `Loading` or a `Skeleton`.
 */
export function Spinner({ label, size = 14 }: { label?: string; size?: 14 | 16 | 20 }) {
  return (
    <span className={styles.spinner}>
      <Icon name="loader" size={size} {...(label ? { label } : {})} />
    </span>
  );
}
