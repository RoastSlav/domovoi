import { useState } from "react";
import { useApprove, useDeny } from "../../api/hooks/actions";
import { ApiFailure, type Action } from "../../api/types";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import styles from "./approvals.module.css";

function failureText(error: unknown): string {
  if (error instanceof ApiFailure && error.status < 500) {
    return error.message;
  }
  return "Something went wrong. Try again.";
}

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
  const approve = useApprove();
  const deny = useDeny();
  const destructive = action.risk === "destructive";
  const matches = typed === action.skill;
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
      <Button onClick={() => deny.mutate(action.id)} disabled={deny.isPending}>
        Deny
      </Button>
      {deny.error && (
        <p className={styles.error} role="alert">
          {failureText(deny.error)}
        </p>
      )}
      {open && (
        <ConfirmDialog
          title={destructive ? `Run destructive action ${action.skill}?` : `Run ${action.skill}?`}
          confirmLabel="Approve"
          confirmVariant={destructive ? "danger" : "primary"}
          confirmDisabled={destructive && !matches}
          busy={approve.isPending}
          onConfirm={onConfirm}
          onCancel={close}
        >
          <p>
            <strong>{action.skill}</strong>
            {args ? ` with ${args}` : ""} runs on the host as soon as you approve.
          </p>
          {destructive && (
            <div className={styles.field}>
              <label htmlFor={`confirm-${action.id}`}>
                Type <code>{action.skill}</code> to confirm
              </label>
              <input
                id={`confirm-${action.id}`}
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
                autoComplete="off"
                spellCheck={false}
              />
            </div>
          )}
          {approve.error && (
            <p className={styles.error} role="alert">
              {failureText(approve.error)}
            </p>
          )}
        </ConfirmDialog>
      )}
    </div>
  );
}
