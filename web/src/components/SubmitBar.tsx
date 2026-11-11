import type { ReactNode } from "react";
import { Button } from "./Button";
import styles from "./form.module.css";

/**
 * The bottom bar of a long form, stuck to the viewport: how many changes are unsaved,
 * Discard, and Save. Save submits the enclosing form unless `onSave` is given.
 *
 * Use it on forms taller than a screen. A short form or dialog has plain buttons.
 */
export function SubmitBar({
  changes,
  onDiscard,
  onSave,
  saving = false,
  saveLabel = "Save",
  disabledReason,
  children,
}: {
  /** How many values differ from what is saved. */
  changes: number;
  /** Extra controls, left of Discard. */
  children?: ReactNode;
  /** Why saving is not possible, other than "nothing changed". */
  disabledReason?: string | undefined;
  onDiscard: () => void;
  onSave?: () => void;
  saveLabel?: string;
  saving?: boolean;
}) {
  const nothing = changes === 0 ? "Nothing has changed." : undefined;
  return (
    <div className={styles.submitBar}>
      <p className={styles.changes} role="status">
        {changes === 0
          ? "No unsaved changes"
          : changes === 1
            ? "1 unsaved change"
            : `${changes} unsaved changes`}
      </p>
      {children}
      <Button onClick={onDiscard} disabledReason={nothing}>
        Discard
      </Button>
      <Button
        variant="primary"
        type={onSave ? "button" : "submit"}
        loading={saving}
        disabledReason={disabledReason ?? nothing}
        {...(onSave ? { onClick: onSave } : {})}
      >
        {saveLabel}
      </Button>
    </div>
  );
}
