import { useState } from "react";
import { useEditSummaryField } from "../../api/hooks/experience";
import { ApiFailure, type SummaryField } from "../../api/types";
import { ConfirmDialog, Field, Textarea, useActionFeedback } from "../../components";
import { summaryFieldLabel } from "../../design/knowledgeText";

/** The longest text a narrative field takes. */
export const MAX_SUMMARY_CHARS = 1000;

/**
 * Edits one narrative field, or writes one that has no revision yet. The text is saved as
 * written by the owner. A stale revision or a bad text reads inside the dialog and keeps what
 * was typed; `onStale` lets the page refetch so the next try names the newest revision.
 */
export function EditSummaryDialog({
  operationId,
  fieldName,
  field,
  initial,
  onClose,
  onStale,
}: {
  field: SummaryField | undefined;
  fieldName: string;
  initial: string;
  onClose: () => void;
  onStale: () => void;
  operationId: string;
}) {
  const [text, setText] = useState(initial);
  const label = summaryFieldLabel(fieldName);
  const edit = useActionFeedback(useEditSummaryField(), {
    success: "Saved your edit.",
    error: "save your edit",
    errorDisplay: "inline",
  });
  const trimmed = text.trim();
  const reason =
    trimmed === ""
      ? "Write something first."
      : text.length > MAX_SUMMARY_CHARS
        ? `The text is over ${MAX_SUMMARY_CHARS} characters. Shorten it.`
        : undefined;
  return (
    <ConfirmDialog
      title={field ? `Edit the ${label.toLowerCase()}` : `Add the ${label.toLowerCase()}`}
      confirmLabel="Save"
      busy={edit.isPending}
      error={edit.error}
      confirmDisabledReason={reason}
      onCancel={onClose}
      onConfirm={() =>
        edit.mutate(
          {
            operation: operationId,
            field: fieldName,
            value: trimmed,
            expectedRevision: field?.revision ?? 0,
          },
          {
            onSuccess: onClose,
            onError: (failure) => {
              if (failure instanceof ApiFailure && failure.status === 409) {
                onStale();
              }
            },
          },
        )
      }
    >
      <Field
        label={label}
        hint={`Up to ${MAX_SUMMARY_CHARS} characters. It is saved as written by you. ${text.length} of ${MAX_SUMMARY_CHARS}.`}
      >
        <Textarea rows={5} value={text} onChange={(event) => setText(event.target.value)} />
      </Field>
    </ConfirmDialog>
  );
}
