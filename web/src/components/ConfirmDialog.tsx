import { useId, useState, type ReactNode } from "react";
import type { DescribedError } from "../design/messages";
import { Button } from "./Button";
import { ErrorDetail } from "./ErrorDetail";
import { InlineMessage } from "./InlineMessage";
import { Dialog } from "./Dialog";
import { Field } from "./Field";
import { TextInput } from "./inputs";
import styles from "./overlay.module.css";

/** What a confirmation dialog asks and does. */
export interface ConfirmDialogProps {
  /** The action is running: the confirm button shows a spinner. */
  busy?: boolean;
  /** Extra content: a form field, or an inline error. */
  children?: ReactNode;
  /** Why confirming is not possible yet. */
  confirmDisabledReason?: string | undefined;
  /** Why the last attempt failed, from `useActionFeedback` with `errorDisplay: "inline"`. */
  error?: DescribedError | undefined;
  /** Repeats the verb: "Revoke agent", not "OK". */
  confirmLabel: string;
  /** What happens if confirmed, in a sentence. */
  consequence?: ReactNode;
  onCancel: () => void;
  onConfirm: () => void;
  /** For very destructive actions: the person must type this exactly before confirming. */
  requireText?: string | undefined;
  /** Names the object: "Revoke agent nas-01?" */
  title: string;
  /** `danger` for something destructive or irreversible. */
  tone?: "primary" | "danger";
  /** Receives what was typed, when `requireText` is set. */
  onTyped?: (text: string) => void;
}

/**
 * A modal yes-or-no question before something destructive or irreversible: the title names
 * the object, `consequence` says what happens, and the confirm button repeats the verb.
 *
 * Do not use it for actions that can be undone: do them and offer Undo in the toast. A
 * failure is shown inside the dialog (`error`), because a toast would sit behind it.
 */
export function ConfirmDialog({
  title,
  consequence,
  children,
  confirmLabel,
  tone = "primary",
  confirmDisabledReason,
  requireText,
  busy = false,
  error,
  onConfirm,
  onCancel,
  onTyped,
}: ConfirmDialogProps) {
  const [typed, setTyped] = useState("");
  const id = useId();
  const reason =
    confirmDisabledReason ??
    (requireText !== undefined && typed !== requireText
      ? `Type ${requireText} to confirm.`
      : undefined);
  return (
    <Dialog
      title={title}
      onClose={onCancel}
      actions={
        <>
          <Button onClick={onCancel}>Cancel</Button>
          <Button variant={tone} loading={busy} disabledReason={reason} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {consequence && <div className={styles.consequence}>{consequence}</div>}
      {requireText !== undefined && (
        <Field
          id={id}
          label={
            <>
              Type <code>{requireText}</code> to confirm
            </>
          }
        >
          <TextInput
            value={typed}
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => {
              setTyped(event.target.value);
              onTyped?.(event.target.value);
            }}
          />
        </Field>
      )}
      {children}
      {error && (
        <InlineMessage tone="danger" title={error.title}>
          <ErrorDetail error={error} />
        </InlineMessage>
      )}
    </Dialog>
  );
}
