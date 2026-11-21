import { useId, useState, type ReactNode } from "react";
import { Icon } from "../design/icons";
import { Count } from "./Count";
import styles from "./visibility.module.css";

/**
 * A labelled toggle that shows its content in place when opened. The content is not rendered
 * until then. It starts open when `reveal` is true and opens (never closes by itself) when
 * `reveal` turns true later, so a selected item inside is never left behind a closed toggle.
 *
 * Use it for technical detail a detail level folds away, or a run of list rows (`as="li"`).
 * Do not use it for a page section (use `Section collapsed`) or for anything a person must see.
 */
export function Disclosure({
  label,
  count,
  summary,
  reveal,
  as = "div",
  children,
}: {
  as?: "div" | "li";
  children: ReactNode;
  count?: number | undefined;
  label: string;
  reveal?: boolean | undefined;
  summary?: string | undefined;
}) {
  const id = useId();
  const [open, setOpen] = useState(reveal === true);
  const [revealed, setRevealed] = useState(reveal === true);
  // Adjust during render: a reveal that turns on opens the block, one that turns off only resets.
  if (reveal === true && !revealed) {
    setRevealed(true);
    setOpen(true);
  } else if (reveal !== true && revealed) {
    setRevealed(false);
  }
  const Root = as;
  return (
    <Root className={styles.disclosure}>
      <button
        type="button"
        className={styles.toggle}
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onClick={() => setOpen((now) => !now)}
      >
        <Icon name={open ? "chevron-down" : "chevron-right"} size={16} />
        <span>{label}</span>
        {count !== undefined && <Count tone="neutral" value={count} label="items" />}
        {summary && <span className={styles.summary}>{summary}</span>}
      </button>
      {open && (
        <div id={id} role="group" aria-label={label} className={styles.body}>
          {children}
        </div>
      )}
    </Root>
  );
}
