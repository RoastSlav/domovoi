import { useNavigate } from "react-router";
import { useDiscardDraft } from "../../api/hooks/policyCommands";
import { ConfirmDialog, useActionFeedback } from "../../components";

/**
 * Asks before the stored draft is thrown away, then discards it, naming the revision the page
 * read, and returns to the policy page.
 * @param revision The draft revision being discarded.
 * @param onClose Closes the dialog without discarding.
 * @param allowLeaving Lets the page navigate away without the unsaved-changes question.
 */
export function DiscardDraft({
  revision,
  onClose,
  allowLeaving,
}: {
  allowLeaving: () => void;
  onClose: () => void;
  revision: number;
}) {
  const navigate = useNavigate();
  const discard = useActionFeedback(useDiscardDraft(), {
    success: "Draft discarded.",
    error: "discard the draft",
    errorDisplay: "inline",
  });
  return (
    <ConfirmDialog
      title="Discard the draft?"
      consequence="The draft and any edits on this page are deleted. The active version is not touched."
      confirmLabel="Discard draft"
      tone="danger"
      busy={discard.isPending}
      error={discard.error}
      onCancel={onClose}
      onConfirm={() =>
        discard.mutate(
          { revision },
          {
            onSuccess: () => {
              allowLeaving();
              void navigate("/policy");
            },
          },
        )
      }
    />
  );
}
