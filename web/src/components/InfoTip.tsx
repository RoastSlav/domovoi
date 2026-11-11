import { useId, useRef, useState, type ReactNode } from "react";
import { docsUrl } from "../design/docs";
import { Icon } from "../design/icons";
import { Popover } from "./Popover";
import styles from "./overlay.module.css";

/**
 * A small info button that opens a longer explanation on click or Enter, so it also works
 * on touch. `learnMore` is a page in the book, like `concepts/actions.html`.
 *
 * Use it for a concept that needs a sentence or two (what a risk level allows, what "known
 * condition" means). A few words on hover are a `Tooltip`; one line under an input is the
 * `Field` hint.
 */
export function InfoTip({
  title,
  children,
  learnMore,
  label,
}: {
  children: ReactNode;
  /** The button's accessible name; "About <title>" unless given. */
  label?: string;
  learnMore?: string;
  title?: string;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const id = useId();
  const name = label ?? (title ? `About ${title}` : "More information");
  return (
    <>
      <button
        ref={button}
        type="button"
        className={styles.infoTip}
        aria-label={name}
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onClick={() => setOpen((now) => !now)}
      >
        <Icon name="info" size={16} />
      </button>
      {open && (
        <Popover anchor={button} id={id} label={title ?? name} onClose={() => setOpen(false)}>
          {title && <p className={styles.popoverTitle}>{title}</p>}
          <div className={styles.popoverBody}>{children}</div>
          {learnMore && (
            <a
              className={styles.learnMore}
              href={docsUrl(learnMore)}
              target="_blank"
              rel="noreferrer"
              aria-label={`Learn more${title ? ` about ${title}` : ""} (opens in a new tab)`}
            >
              Learn more
              <Icon name="external" size={14} />
            </a>
          )}
        </Popover>
      )}
    </>
  );
}
