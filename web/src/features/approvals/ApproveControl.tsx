import { useState } from "react";
import { useApprove, useDeny } from "../../api/hooks/actions";
import type { Action } from "../../api/types";
import { Button, Cluster, ConfirmDialog, useActionFeedback } from "../../components";

function argsText(action: Action): string {
  return Object.entries(action.args)
    .map(([name, value]) => `${name}=${value}`)
    .join(", ");
}

/** What approving does, in a sentence that names the skill, its arguments and the host. */
function consequence(action: Action, host: string | undefined): string {
  const args = argsText(action);
  const what = args ? `${action.skill} with ${args}` : action.skill;
  const where = host ? `on ${host}` : "on the host";
  return action.risk === "destructive"
    ? `${what} runs ${where} as soon as you approve. It can lose data, and it cannot be undone.`
    : `${what} runs ${where} as soon as you approve.`;
}

/**
 * Approve and Deny for a waiting action. Approving always asks first, saying what will run
 * and where; a destructive skill also needs its name typed exactly, which is sent to the hub
 * as `confirm_text` (INV-6). Denying is one click: nothing runs, and triage can ask again.
 */
export function ApproveControl({ action, host }: { action: Action; host?: string | undefined }) {
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const approve = useActionFeedback(useApprove(), {
    success: `Approved ${action.skill}. It runs now.`,
    error: "approve the action",
    errorDisplay: "inline",
  });
  const deny = useActionFeedback(useDeny(), {
    success: `Denied ${action.skill}. Nothing ran.`,
    error: "deny the action",
  });
  const destructive = action.risk === "destructive";

  const close = () => {
    setOpen(false);
    setTyped("");
    approve.reset();
  };

  const onConfirm = () =>
    approve.mutate(destructive ? { id: action.id, confirmText: typed } : { id: action.id }, {
      onSuccess: close,
    });

  return (
    <Cluster>
      <Button
        variant="primary"
        hint="Asks you to confirm, then runs the skill on the host."
        onClick={() => setOpen(true)}
      >
        Approve
      </Button>
      <Button
        hint="Declines it. Nothing runs, and triage can propose it again."
        onClick={() => deny.mutate(action.id)}
        loading={deny.isPending}
      >
        Deny
      </Button>
      {open && (
        <ConfirmDialog
          title={destructive ? `Run destructive action ${action.skill}?` : `Run ${action.skill}?`}
          consequence={consequence(action, host)}
          confirmLabel="Approve"
          tone={destructive ? "danger" : "primary"}
          requireText={destructive ? action.skill : undefined}
          onTyped={setTyped}
          busy={approve.isPending}
          error={approve.error}
          onConfirm={onConfirm}
          onCancel={close}
        />
      )}
    </Cluster>
  );
}
