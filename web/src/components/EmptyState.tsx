import type { ReactNode } from "react";
import { Icon, type IconName } from "../design/icons";
import styles from "./state.module.css";

/**
 * What a view shows when it has nothing in it: a dashed box with an icon tile, why it is
 * empty (a body up to 48 characters wide), and the next step.
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
      {icon && (
        <span className={styles.emptyIcon}>
          <Icon name={icon} size={20} />
        </span>
      )}
      <p className={styles.emptyTitle}>{title}</p>
      {children && <div className={styles.emptyBody}>{children}</div>}
      {action && <div className={styles.emptyAction}>{action}</div>}
    </div>
  );
}
