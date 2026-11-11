import type { ReactNode } from "react";
import { DocsLink } from "./DocsLink";
import styles from "./layout.module.css";

/**
 * The top of a page: its title (the page's `h1`), an optional row of pills and tags under
 * it, one or two sentences saying what the page is for, and the page's actions on the right.
 * `docs` is a page in the book, like `concepts/incidents.html`, and adds "Learn more";
 * `topic` names the page in that link's accessible name.
 *
 * Use it once at the top of every page. Do not write an `h1` anywhere else.
 */
export function PageHeader({
  title,
  intro,
  docs,
  topic,
  actions,
  meta,
}: {
  /** The page's main actions, right of the title. */
  actions?: ReactNode;
  docs?: string;
  intro?: ReactNode;
  /** Pills and tags under the title. */
  meta?: ReactNode;
  title: ReactNode;
  /** What the docs link is about; the lower-cased title unless given. */
  topic?: string;
}) {
  const about = topic ?? (typeof title === "string" ? title.toLowerCase() : "this page");
  return (
    <header className={styles.pageHeader}>
      <div className={styles.pageHeaderText}>
        <h1 className={styles.pageTitle}>{title}</h1>
        {meta && <div className={styles.pageMeta}>{meta}</div>}
        {intro && (
          <p className={styles.intro}>
            {intro}
            {docs && (
              <>
                {" "}
                <DocsLink docs={docs} topic={about} />
              </>
            )}
          </p>
        )}
      </div>
      {actions && <div className={styles.headerActions}>{actions}</div>}
    </header>
  );
}
