import type { ReactNode } from "react";
import { Button } from "./Button";
import styles from "./submitbar.module.css";

/**
 * The floating unsaved-changes bar of a long form: how many changes are unsaved, an optional
 * note (a tag such as "Restarts the hub"), Discard and Save. It slides up at the bottom
 * centre of the content column while there is something to save, and renders nothing when
 * `changes` is 0 (unless `alwaysShow`). Save submits the enclosing form unless `onSave` is
 * given.
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
  alwaysShow = false,
  children,
}: {
  /** Show the bar even when nothing has changed (Save and Discard then say so). */
  alwaysShow?: boolean;
  /** How many values differ from what is saved. */
  changes: number;
  /** A note beside the count: a `Tag` saying what saving does. */
  children?: ReactNode;
  /** Why saving is not possible, other than "nothing changed". */
  disabledReason?: string | undefined;
  onDiscard: () => void;
  onSave?: () => void;
  saveLabel?: string;
  saving?: boolean;
}) {
  if (changes === 0 && !alwaysShow) {
    return null;
  }
  const nothing = changes === 0 ? "Nothing has changed." : undefined;
  return (
    <div className={styles.dock}>
      <div className={styles.bar}>
        <p className={styles.changes} role="status">
          {changes === 0
            ? "No unsaved changes"
            : changes === 1
              ? "1 unsaved change"
              : `${changes} unsaved changes`}
        </p>
        {children}
        <div className={styles.actions}>
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
      </div>
    </div>
  );
}
