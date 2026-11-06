import type { ReactNode } from "react";
import styles from "./ui.module.css";

/**
 * The published user docs. The hub serves these pages, so the book a link points at can be ahead
 * of the version running here, and is unreachable when the hub is offline: the words on the page
 * carry the meaning, and the link is the way to the longer version.
 */
const DOCS = "https://RoastSlav.github.io/domovoi/";

/**
 * One or two sentences under a page heading, saying what the page is for. `docs` is a path in the
 * book, such as `concepts/incidents.html`, and adds a "Learn more" link; `topic` names the page,
 * so the link says what it leads to and that it opens a new tab.
 */
export function PageIntro({
  topic,
  docs,
  children,
}: {
  topic: string;
  docs?: string;
  children: ReactNode;
}) {
  return (
    <p className={styles.intro}>
      {children}
      {docs && (
        <a
          href={`${DOCS}${docs}`}
          target="_blank"
          rel="noreferrer"
          aria-label={`Learn more about ${topic} (opens in a new tab)`}
        >
          Learn more
        </a>
      )}
    </p>
  );
}
