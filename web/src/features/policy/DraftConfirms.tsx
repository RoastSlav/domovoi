import type { PolicyRule } from "../../api/types";
import { ConfirmDialog } from "../../components";

/**
 * The questions the draft page asks before it throws away or removes something: discarding
 * many edits, reloading over edits, and removing a locked rule. Only the open one renders.
 * @param changes How many rules differ from the stored draft.
 * @param dropping Discard of the edits is being asked.
 * @param reloading Reload over the edits is being asked.
 * @param removing The locked rule whose removal is being asked.
 * @param onDrop Discards the edits.
 * @param onReload Reloads the stored draft.
 * @param onRemove Removes the rule with this id.
 * @param onClose Closes whichever question is open.
 */
export function DraftConfirms({
  changes,
  dropping,
  reloading,
  removing,
  onDrop,
  onReload,
  onRemove,
  onClose,
}: {
  changes: number;
  dropping: boolean;
  onClose: () => void;
  onDrop: () => void;
  onReload: () => void;
  onRemove: (id: string) => void;
  reloading: boolean;
  removing: PolicyRule | undefined;
}) {
  return (
    <>
      {dropping && (
        <ConfirmDialog
          title="Discard your changes?"
          consequence={`The ${changes} unsaved changes on this page are lost. The stored draft stays as it is.`}
          confirmLabel="Discard changes"
          tone="danger"
          onCancel={onClose}
          onConfirm={onDrop}
        />
      )}
      {reloading && (
        <ConfirmDialog
          title="Discard your edits and reload?"
          consequence="Your unsaved edits are lost and the page shows the draft as it is stored now."
          confirmLabel="Discard edits and reload"
          tone="danger"
          onCancel={onClose}
          onConfirm={onReload}
        />
      )}
      {removing && (
        <ConfirmDialog
          title={`Remove ${removing.id}?`}
          consequence="This rule is locked. Saving the draft without it asks for your password and shows what it would change. Nothing is stored before then."
          confirmLabel="Remove rule"
          tone="danger"
          onCancel={onClose}
          onConfirm={() => onRemove(removing.id)}
        />
      )}
    </>
  );
}
