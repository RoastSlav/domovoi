import { useState } from "react";
import { useReleaseBlock } from "../../api/hooks/containment";
import { ConfirmDialog, useActionFeedback } from "../../components";
import { PasswordRow } from "./PasswordRow";

/**
 * Takes a block away before its time runs out. It asks for the password and the source typed
 * exactly, so an early release is never a stray click.
 */
export function ReleaseDialog({
  operationId,
  step,
  source,
  onClose,
}: {
  onClose: () => void;
  operationId: string;
  source: string;
  step: string;
}) {
  const [password, setPassword] = useState("");
  const [typed, setTyped] = useState("");
  const release = useActionFeedback(useReleaseBlock(operationId), {
    success: `Released ${source}. It is no longer blocked.`,
    error: "release the block",
    errorDisplay: "inline",
  });
  const confirm = () => {
    release.mutate(
      { step, password, confirmSource: typed },
      {
        onSuccess: onClose,
        onError: () => setPassword(""),
      },
    );
  };
  return (
    <ConfirmDialog
      title={`Release ${source}?`}
      consequence="The helper removes the block now. The source can reach this host again."
      confirmLabel="Release block"
      requireText={source}
      onTyped={setTyped}
      confirmDisabledReason={password === "" ? "Enter your password first." : undefined}
      busy={release.isPending}
      error={release.error}
      onConfirm={confirm}
      onCancel={onClose}
    >
      <PasswordRow value={password} onChange={setPassword} />
    </ConfirmDialog>
  );
}
