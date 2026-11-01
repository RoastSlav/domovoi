import type { ReactNode } from "react";
import styles from "./ui.module.css";

/** What a list shows when it has nothing in it, or a page while it cannot load. */
export function EmptyState({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className={styles.empty} role="status">
      <p className={styles.emptyTitle}>{title}</p>
      {children}
    </div>
  );
}
