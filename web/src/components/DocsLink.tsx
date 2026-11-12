import { docsUrl } from "../design/docs";
import { Icon } from "../design/icons";
import styles from "./layout.module.css";

/**
 * A "Learn more" link to a page of the book, opening in a new tab. `docs` is a path like
 * `concepts/incidents.html`; `topic` names what the page is about, so the link's accessible
 * name says where it leads.
 *
 * Use it after a sentence that already carries the meaning: the book may be unreachable.
 * `PageHeader` and `InfoTip` add their own; use this one inside a section's text.
 */
export function DocsLink({ docs, topic }: { docs: string; topic: string }) {
  return (
    <a
      href={docsUrl(docs)}
      target="_blank"
      rel="noreferrer"
      className={styles.docsLink}
      aria-label={`Learn more about ${topic} (opens in a new tab)`}
    >
      Learn more
      <Icon name="external" size={14} />
    </a>
  );
}
