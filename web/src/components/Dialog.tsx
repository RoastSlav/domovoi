import { useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import styles from "./ui.module.css";

const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), select, textarea, a[href], [tabindex]:not([tabindex="-1"])';

/** What a dialog shows. */
export interface DialogProps {
  title: string;
  children?: ReactNode;
  /** The buttons along the bottom. */
  actions: ReactNode;
  /** Called on Escape. */
  onClose: () => void;
  wide?: boolean;
}

/**
 * A modal dialog, shown while mounted: labelled by its title, keeping focus inside until
 * it closes, closing on Escape, and returning focus to where it was.
 */
export function Dialog({ title, children, actions, onClose, wide = false }: DialogProps) {
  const titleId = useId();
  const dialog = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.querySelector<HTMLElement>(FOCUSABLE)?.focus();
    return () => opener?.focus();
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== "Tab" || !dialog.current) {
      return;
    }
    const items = [...dialog.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = items[0];
    const last = items.at(-1);
    if (!first || !last) {
      return;
    }
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div className={styles.backdrop}>
      <div
        ref={dialog}
        className={wide ? `${styles.dialog} ${styles.wide}` : styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onKeyDown={onKeyDown}
      >
        <h2 id={titleId}>{title}</h2>
        {children}
        <div className={styles.dialogActions}>{actions}</div>
      </div>
    </div>
  );
}
