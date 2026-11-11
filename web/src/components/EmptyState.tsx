import type { ReactNode } from "react";
import { Icon, type IconName } from "../design/icons";
import styles from "./state.module.css";

/**
 * What a view shows when it has nothing in it: why it is empty, and the next step.
 *
 * Use it only when the data loaded and there is none. A failed load is a `LoadError`, never
 * an empty state.
 */
export function EmptyState({
  icon,
  title,
  children,
  action,
}: {
  /** The next step, usually one `Button` or link. */
  action?: ReactNode;
  children?: ReactNode;
  icon?: IconName;
  title: string;
}) {
  return (
    <div className={styles.empty} role="status">
      {icon && <Icon name={icon} size={20} />}
      <p className={styles.emptyTitle}>{title}</p>
      {children && <div className={styles.emptyBody}>{children}</div>}
      {action}
    </div>
  );
}
