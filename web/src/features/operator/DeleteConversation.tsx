import { useState } from "react";
import { useDeleteConversation } from "../../api/hooks/conversations";
import { Button, ConfirmDialog, useActionFeedback } from "../../components";

/**
 * Deletes a conversation after a confirmation that names it. Disabled with its reason while
 * an answer is being written, because the hub refuses then too.
 */
export function DeleteConversation({
  id,
  title,
  pending,
  onDeleted,
}: {
  id: string;
  onDeleted: () => void;
  pending: boolean;
  title: string;
}) {
  const [asking, setAsking] = useState(false);
  const remove = useActionFeedback(useDeleteConversation(), {
    success: "Conversation deleted",
    error: "delete the conversation",
    errorDisplay: "inline",
  });
  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        icon="trash"
        disabledReason={pending ? "An answer is being written. Stop it first." : undefined}
        onClick={() => setAsking(true)}
      >
        Delete
      </Button>
      {asking && (
        <ConfirmDialog
          title={`Delete “${title || "New conversation"}”?`}
          consequence="Its messages are removed. Backups keep them until they rotate."
          confirmLabel="Delete conversation"
          tone="danger"
          busy={remove.isPending}
          error={remove.error}
          onCancel={() => setAsking(false)}
          onConfirm={() =>
            remove.mutate(id, {
              onSuccess: () => {
                setAsking(false);
                onDeleted();
              },
            })
          }
        />
      )}
    </>
  );
}
