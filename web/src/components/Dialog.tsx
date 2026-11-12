import { useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import styles from "./overlay.module.css";

const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

/** What a dialog shows. */
export interface DialogProps {
  /** The buttons along the bottom. */
  actions: ReactNode;
  children?: ReactNode;
  /** Called on Escape. */
  onClose: () => void;
  title: string;
  wide?: boolean;
}

/**
 * A modal dialog, shown while mounted, on the native `<dialog>`: the overlay surface with a
 * 16 px header, the content, and a footer with the buttons at the right, over a blurred
 * scrim. It is labelled by its title, keeps focus inside until it closes, closes on Escape,
 * holds the page still behind it and returns focus to where it was.
 *
 * Use it for a question or a short form that must be answered before going on. Do not use
 * it for an explanation (`InfoTip`) or for the result of an action (toast).
 */
export function Dialog({ title, children, actions, onClose, wide = false }: DialogProps) {
  const titleId = useId();
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const node = dialog.current;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (node && !node.open) {
      node.showModal();
    }
    node?.querySelector<HTMLElement>(FOCUSABLE)?.focus();
    return () => {
      if (node?.open) {
        node.close();
      }
      opener?.focus();
    };
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLDialogElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
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
    <dialog
      ref={dialog}
      className={wide ? `${styles.dialog} ${styles.wide}` : styles.dialog}
      aria-modal="true"
      aria-labelledby={titleId}
      onKeyDown={onKeyDown}
      onCancel={(event) => {
        // Escape is handled above; this keeps the browser from closing it behind React's back.
        event.preventDefault();
        onClose();
      }}
    >
      <div className={styles.dialogBody}>
        <h2 id={titleId} className={styles.dialogTitle}>
          {title}
        </h2>
        {children}
      </div>
      <div className={styles.dialogActions}>{actions}</div>
    </dialog>
  );
}
