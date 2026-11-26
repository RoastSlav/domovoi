import { useState } from "react";
import { useApproveStep, useDenyStep } from "../../api/hooks/operationControls";
import type { ContainmentStep } from "../../api/types";
import {
  Button,
  Checkbox,
  Cluster,
  ConfirmDialog,
  Stack,
  Text,
  Visible,
  formatRemaining,
  useActionFeedback,
  useRemaining,
} from "../../components";
import { TYPED_SOURCE_HINT } from "../../design/containmentText";
import type { ApprovalPlan } from "../approvals/approvalPlan";
import { UnverifiedNote } from "./ContainmentFacts";
import { PasswordRow } from "./PasswordRow";

const EXPIRED_REASON = "This request expired; it can be proposed again.";
const CHANGED_REASON = "The plan changed while you were looking. Close this and review it again.";

/**
 * Approve and Deny for a containment step the hub could not verify. The warning is always on
 * the card. Approving needs your password, the source typed exactly and a tick that you know
 * coverage was not verified; the plan hash shown when the dialog opened is what is sent.
 */
export function UnverifiedApproveControl({
  plan,
  operationId,
  step,
}: {
  operationId: string;
  plan: ApprovalPlan;
  step: ContainmentStep;
}) {
  const [shownHash, setShownHash] = useState<string | null>(null);
  const [password, setPassword] = useState("");
  const [acknowledged, setAcknowledged] = useState(false);
  const approve = useActionFeedback(useApproveStep(operationId), {
    success: `Approved blocking ${step.source}. It is applied now.`,
    error: "approve the block",
    errorDisplay: "inline",
  });
  const deny = useActionFeedback(useDenyStep(operationId), {
    success: `Denied blocking ${step.source}. Nothing changed.`,
    error: "deny the block",
  });
  const expired = useRemaining(plan.expiresAt) === 0;
  const changed = shownHash !== null && shownHash !== plan.planHash;

  const close = () => {
    setShownHash(null);
    setPassword("");
    setAcknowledged(false);
    approve.reset();
  };

  const reason = changed
    ? CHANGED_REASON
    : !acknowledged
      ? "Tick the box to say you understand."
      : password === ""
        ? "Enter your password first."
        : undefined;

  const onConfirm = () => {
    if (shownHash === null) {
      return;
    }
    approve.mutate(
      { step: plan.id, planHash: shownHash, password, unverifiedAcknowledged: acknowledged },
      { onSuccess: close, onError: () => setPassword("") },
    );
  };

  return (
    <Stack gap={3}>
      <Visible category="P0" id="containment.unverified">
        <UnverifiedNote step={step} />
      </Visible>
      <Cluster>
        <Button
          variant="danger"
          hint="Asks for your password and the source, then blocks it."
          disabledReason={expired ? EXPIRED_REASON : undefined}
          onClick={() => setShownHash(plan.planHash)}
        >
          Approve
        </Button>
        <Button
          variant="ghost"
          hint="Declines it. Nothing changes on the host."
          onClick={() => deny.mutate({ step: plan.id })}
          loading={deny.isPending}
        >
          Deny
        </Button>
      </Cluster>
      {shownHash !== null && (
        <ConfirmDialog
          title={`Block ${step.source} without verified coverage?`}
          consequence={`The helper drops traffic from ${step.source} for ${formatRemaining(step.ttl_s * 1000)}. The hub could not verify it leaves your management paths alone.`}
          confirmLabel="Approve block"
          tone="danger"
          requireText={step.source}
          confirmDisabledReason={reason}
          busy={approve.isPending}
          error={approve.error}
          onConfirm={onConfirm}
          onCancel={close}
        >
          <Stack gap={3}>
            <UnverifiedNote step={step} />
            <Text block muted size="sm">
              {TYPED_SOURCE_HINT}
            </Text>
            <Checkbox
              checked={acknowledged}
              onChange={setAcknowledged}
              label="I understand the hub could not verify coverage of my management paths"
            />
            <PasswordRow value={password} onChange={setPassword} />
          </Stack>
        </ConfirmDialog>
      )}
    </Stack>
  );
}
