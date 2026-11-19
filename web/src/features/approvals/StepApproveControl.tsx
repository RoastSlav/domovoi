import { useState } from "react";
import { useApproveStep, useDenyStep } from "../../api/hooks/operationControls";
import {
  Button,
  Cluster,
  ConfirmDialog,
  InlineMessage,
  useActionFeedback,
  useRemaining,
} from "../../components";
import type { ApprovalPlan } from "./approvalPlan";
import styles from "./approvals.module.css";

const EXPIRED_REASON = "This request expired; triage can propose it again.";
const CHANGED_REASON = "The plan changed while you were looking. Close this and review it again.";

function consequence(plan: ApprovalPlan): string {
  const args = Object.entries(plan.args)
    .map(([name, value]) => `${name}=${value}`)
    .join(", ");
  const what = args ? `${plan.skill} with ${args}` : plan.skill;
  const where = plan.host ? `on ${plan.host}` : "on the host";
  const base = `${what} runs ${where} as soon as you approve.`;
  if (plan.risk === "unknown") {
    return `${base} Its risk is unknown, so it is treated as destructive.`;
  }
  return plan.confirmRequired ? `${base} It can lose data, and it cannot be undone.` : base;
}

/**
 * Approve and Deny for a waiting operation step. Approving asks first and sends the plan hash
 * the card showed when the dialog opened; a step that needs confirmation also needs its skill
 * name typed. Deny is one click. Nothing here runs without a click.
 */
export function StepApproveControl({
  plan,
  operationId,
  emphasis = "primary",
}: {
  plan: ApprovalPlan;
  operationId: string;
  emphasis?: "primary" | "secondary";
}) {
  const [shownHash, setShownHash] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const approve = useActionFeedback(useApproveStep(operationId), {
    success: `Approved ${plan.skill}. It runs now.`,
    error: "approve the action",
    errorDisplay: "inline",
  });
  const deny = useActionFeedback(useDenyStep(operationId), {
    success: `Denied ${plan.skill}. Nothing ran.`,
    error: "deny the action",
  });
  const expired = useRemaining(plan.expiresAt) === 0;
  const danger = plan.confirmRequired;
  const changed = shownHash !== null && shownHash !== plan.planHash;

  const close = () => {
    setShownHash(null);
    setTyped("");
    approve.reset();
  };

  const onConfirm = () => {
    if (shownHash === null) {
      return;
    }
    approve.mutate(
      {
        step: plan.id,
        planHash: shownHash,
        ...(plan.confirmRequired ? { confirmText: typed } : {}),
      },
      { onSuccess: close },
    );
  };

  return (
    <Cluster>
      <Button
        variant={danger ? "danger" : emphasis}
        {...(!danger && emphasis === "secondary"
          ? { icon: "check" as const, className: styles.approveIcon }
          : {})}
        hint="Asks you to confirm, then runs the skill on the host."
        disabledReason={expired ? EXPIRED_REASON : undefined}
        onClick={() => setShownHash(plan.planHash)}
      >
        Approve
      </Button>
      <Button
        variant="ghost"
        hint="Declines it. Nothing runs, and triage can propose it again."
        onClick={() => deny.mutate({ step: plan.id })}
        loading={deny.isPending}
      >
        Deny
      </Button>
      {shownHash !== null && (
        <ConfirmDialog
          title={danger ? `Run destructive action ${plan.skill}?` : `Run ${plan.skill}?`}
          consequence={consequence(plan)}
          confirmLabel="Approve"
          tone={danger ? "danger" : "primary"}
          requireText={plan.confirmRequired ? plan.confirmText : undefined}
          confirmDisabledReason={changed ? CHANGED_REASON : undefined}
          onTyped={setTyped}
          busy={approve.isPending}
          error={approve.error}
          onConfirm={onConfirm}
          onCancel={close}
        >
          {changed && (
            <InlineMessage tone="warning" title="The plan changed">
              {CHANGED_REASON}
            </InlineMessage>
          )}
        </ConfirmDialog>
      )}
    </Cluster>
  );
}
