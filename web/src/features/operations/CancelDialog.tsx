import { useState } from "react";
import { useCancelOperation } from "../../api/hooks/operationControls";
import { ConfirmDialog, Field, Textarea, useActionFeedback } from "../../components";

const MAX_REASON = 500;

/**
 * Asks before cancelling an operation, with an optional reason kept in the history. A
 * failure shows inside the dialog; success closes it.
 */
export function CancelDialog({
  operationId,
  goal,
  onClose,
}: {
  operationId: string;
  goal: string;
  onClose: () => void;
}) {
  const [reason, setReason] = useState("");
  const cancel = useActionFeedback(useCancelOperation(operationId), {
    success: "Operation cancelled.",
    error: "cancel the operation",
    errorDisplay: "inline",
  });
  const trimmed = reason.trim();
  const tooLong = reason.length > MAX_REASON;
  return (
    <ConfirmDialog
      title="Cancel this operation?"
      tone="danger"
      consequence={`“${goal}” stops. Steps that have not started are cancelled. An action already running on a host is not recalled; its result is still recorded.`}
      confirmLabel="Cancel operation"
      busy={cancel.isPending}
      error={cancel.error}
      confirmDisabledReason={
        tooLong ? `The reason is over ${MAX_REASON} characters. Shorten it.` : undefined
      }
      onCancel={onClose}
      onConfirm={() => cancel.mutate(trimmed ? { reason: trimmed } : {}, { onSuccess: onClose })}
    >
      <Field
        label="Reason (optional)"
        hint={`Kept in the history. Do not paste secrets. ${reason.length} of ${MAX_REASON} characters.`}
      >
        <Textarea value={reason} onChange={(event) => setReason(event.target.value)} rows={3} />
      </Field>
    </ConfirmDialog>
  );
}
