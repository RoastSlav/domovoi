import { useId, type ReactNode } from "react";
import styles from "./layout.module.css";

/**
 * A titled part of a page: an `h2` with the brown accent, an optional description under it,
 * actions on the right, then the content. The section is labelled by its title.
 *
 * Use it to divide a page. Do not nest sections; inside one, group with `Card` or `Stack`.
 */
export function Section({
  title,
  description,
  actions,
  children,
}: {
  actions?: ReactNode;
  children: ReactNode;
  description?: ReactNode;
  title: string;
}) {
  const id = useId();
  return (
    <section className={styles.section} aria-labelledby={id}>
      <div className={styles.sectionHead}>
        <h2 className={styles.sectionTitle} id={id}>
          {title}
        </h2>
        {actions && <div className={styles.headerActions}>{actions}</div>}
      </div>
      {description && <p className={styles.sectionDescription}>{description}</p>}
      {children}
    </section>
  );
}
