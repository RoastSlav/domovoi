import type { ReactNode } from "react";
import styles from "./ui.module.css";

/** The published user docs, so every "Learn more" link points at the book. */
const DOCS = "https://RoastSlav.github.io/domovoi/";

/**
 * One or two sentences under a page heading, saying what the page is for. `docs` is a path
 * in the book, such as `concepts/incidents.html`, and adds a "Learn more" link.
 */
export function PageIntro({ children, docs }: { children: ReactNode; docs?: string }) {
  return (
    <p className={styles.intro}>
      {children}
      {docs && (
        <a href={`${DOCS}${docs}`} target="_blank" rel="noreferrer">
          Learn more
        </a>
      )}
    </p>
  );
}
