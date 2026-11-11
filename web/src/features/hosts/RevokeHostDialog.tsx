import { useRevoke } from "../../api/hooks/hosts";
import type { Host } from "../../api/types";
import { ConfirmDialog, useActionFeedback } from "../../components";

/**
 * Asks before revoking a host's agent: names the host and what happens, and needs its name
 * typed. The result is a toast once the dialog has closed; a failure stays in the dialog.
 */
export function RevokeHostDialog({ host, onClose }: { host: Host; onClose: () => void }) {
  const revoke = useActionFeedback(useRevoke(), {
    success: (_, name) => `Agent on ${name} revoked`,
    error: "revoke the agent",
    errorDisplay: "inline",
  });
  const close = () => {
    revoke.reset();
    onClose();
  };
  return (
    <ConfirmDialog
      title={`Revoke the agent on ${host.name}?`}
      consequence={`The agent on ${host.name} disconnects now and its key stops working. Checks on it stop until you enroll it again with a new invite.`}
      confirmLabel="Revoke agent"
      tone="danger"
      requireText={host.name}
      busy={revoke.isPending}
      error={revoke.error}
      onConfirm={() => revoke.mutate(host.name, { onSuccess: close })}
      onCancel={close}
    />
  );
}
