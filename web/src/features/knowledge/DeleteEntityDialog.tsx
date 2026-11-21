import { useQueryClient } from "@tanstack/react-query";
import { useDeleteEntity } from "../../api/hooks/knowledgeWrites";
import { keys } from "../../api/queryClient";
import type { KnowledgeEntity } from "../../api/types";
import { ConfirmDialog, useActionFeedback } from "../../components";

/** Asks before deleting an entity; a 409 (still in use, or changed meanwhile) reads inside. */
export function DeleteEntityDialog({
  entity,
  onClose,
  onDeleted,
}: {
  entity: KnowledgeEntity;
  onClose: () => void;
  onDeleted?: () => void;
}) {
  const client = useQueryClient();
  const feedback = useActionFeedback(useDeleteEntity(), {
    success: "Entity deleted.",
    error: "delete the entity",
    errorDisplay: "inline",
  });
  return (
    <ConfirmDialog
      title={`Delete ${entity.name}?`}
      consequence="It must not be used by any live fact or relation."
      confirmLabel="Delete"
      tone="danger"
      busy={feedback.isPending}
      error={feedback.error}
      onCancel={onClose}
      onConfirm={() =>
        feedback.mutate(
          { id: entity.id, expectedVersion: entity.version },
          {
            onSuccess: () => {
              onClose();
              onDeleted?.();
            },
            onError: () => void client.invalidateQueries({ queryKey: keys.knowledge }),
          },
        )
      }
    />
  );
}
