import { useDeletePackage } from "../../api/hooks/packages";
import { ConfirmDialog, useActionFeedback } from "../../components";
import { DELETE_CONSEQUENCE } from "./packageText";

/**
 * Asks before a package leaves the hub's store. Copies already installed on hosts stay; a
 * running install that still needs it refuses in words inside the dialog.
 */
export function DeletePackageDialog({
  name,
  version,
  onCancel,
  onDeleted,
}: {
  name: string;
  onCancel: () => void;
  onDeleted: () => void;
  version: string;
}) {
  const remove = useActionFeedback(useDeletePackage(), {
    error: "delete the package",
    errorDisplay: "inline",
    success: `Deleted ${name} ${version} from the store`,
  });
  return (
    <ConfirmDialog
      title={`Delete ${name} ${version} from the store?`}
      consequence={DELETE_CONSEQUENCE}
      confirmLabel="Delete"
      tone="danger"
      busy={remove.isPending}
      error={remove.error}
      onCancel={onCancel}
      onConfirm={() => remove.mutate({ name, version }, { onSuccess: onDeleted })}
    />
  );
}
