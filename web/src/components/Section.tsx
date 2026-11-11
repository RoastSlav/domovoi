import { useId, useState, type ReactNode } from "react";
import { Icon } from "../design/icons";
import styles from "./layout.module.css";

/**
 * A titled part of a page: an `h2` with the brown accent, an optional description under it,
 * actions on the right, then the content. The section is labelled by its title.
 *
 * `collapsed` makes the title a button that shows and hides the content, starting closed
 * when `true`. The content is not rendered while closed, so what it loads waits until it is
 * opened. Use that for history and other things most visits do not need.
 *
 * Use it to divide a page. Do not nest sections; inside one, group with `Card` or `Stack`.
 */
export function Section({
  title,
  description,
  actions,
  collapsed,
  children,
}: {
  actions?: ReactNode;
  children: ReactNode;
  /** Set (true or false) to make the section collapsible; `true` starts it closed. */
  collapsed?: boolean;
  description?: ReactNode;
  title: string;
}) {
  const id = useId();
  const [open, setOpen] = useState(collapsed !== true);
  const collapsible = collapsed !== undefined;
  return (
    <section className={styles.section} aria-labelledby={id}>
      <div className={styles.sectionHead}>
        <h2 className={styles.sectionTitle} id={id}>
          {collapsible ? (
            <button
              type="button"
              className={styles.sectionToggle}
              aria-expanded={open}
              onClick={() => setOpen((now) => !now)}
            >
              <Icon name={open ? "chevron-down" : "chevron-right"} />
              {title}
            </button>
          ) : (
            title
          )}
        </h2>
        {actions && <div className={styles.headerActions}>{actions}</div>}
      </div>
      {open && (
        <>
          {description && <p className={styles.sectionDescription}>{description}</p>}
          {children}
        </>
      )}
    </section>
  );
}
