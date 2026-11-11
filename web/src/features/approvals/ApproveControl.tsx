import { useState } from "react";
import { useApprove, useDeny } from "../../api/hooks/actions";
import type { Action } from "../../api/types";
import { Button, ConfirmDialog, useActionFeedback } from "../../components";
import styles from "./approvals.module.css";

function argsText(action: Action): string {
  return Object.entries(action.args)
    .map(([name, value]) => `${name}=${value}`)
    .join(", ");
}

/**
 * Approve and Deny for a proposed action. Read and write skills ask for a simple
 * confirmation; destructive skills need their name typed exactly, which is sent to the hub
 * as `confirm_text` (INV-6).
 */
export function ApproveControl({ action }: { action: Action }) {
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
  const args = argsText(action);

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
    <div className={styles.controls}>
      <Button variant={destructive ? "danger" : "primary"} onClick={() => setOpen(true)}>
        Approve
      </Button>
      <Button onClick={() => deny.mutate(action.id)} loading={deny.isPending}>
        Deny
      </Button>
      {open && (
        <ConfirmDialog
          title={destructive ? `Run destructive action ${action.skill}?` : `Run ${action.skill}?`}
          consequence={
            <>
              <strong>{action.skill}</strong>
              {args ? ` with ${args}` : ""} runs on the host as soon as you approve.
            </>
          }
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
    </div>
  );
}
