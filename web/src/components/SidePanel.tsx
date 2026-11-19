import type { ReactNode, Ref } from "react";
import { IconButton } from "./IconButton";
import styles from "./sidePanel.module.css";

/**
 * The chrome of one panel in the shell's right-hand slot: a heading, header actions, a close
 * button and a body that scrolls on its own. It knows nothing about the slot; the slot host
 * decides where it goes and what closing it means.
 *
 * Use it only through the slot (`RightSlotSurface`). Dialogs and popovers are other things.
 */
export function SidePanel({
  title,
  onClose,
  actions,
  headingRef,
  mode = "side",
  children,
}: {
  title: string;
  onClose: () => void;
  /** Controls left of the close button. */
  actions?: ReactNode;
  /** Lets the owner move focus to the heading when the panel opens. */
  headingRef?: Ref<HTMLHeadingElement>;
  /** `sheet` fills the window; `side` sits beside the page. */
  mode?: "side" | "sheet";
  children: ReactNode;
}) {
  return (
    <div className={styles.panel} data-mode={mode}>
      <div className={styles.header}>
        <h2 ref={headingRef} tabIndex={-1} className={styles.title}>
          {title}
        </h2>
        <div className={styles.actions}>
          {actions}
          <IconButton icon="close" label={`Close ${title}`} size="sm" onClick={onClose} />
        </div>
      </div>
      <div className={styles.body}>{children}</div>
    </div>
  );
}
