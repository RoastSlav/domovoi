import { useOperationStep } from "../../api/hooks/operations";
import { QueryBoundary, Skeleton } from "../../components";
import type { ApprovalPlan } from "../approvals/approvalPlan";
import { StepApproveControl } from "../approvals/StepApproveControl";
import { UnverifiedApproveControl } from "./UnverifiedApproveControl";

/**
 * The decision for a containment step. It reads the step first: a block whose coverage the
 * hub verified is approved like any other step; one it could not verify gets the card with
 * the warning, the password, the typed source and the tick. A step that did not load is an
 * error with Retry, never an ordinary Approve.
 */
export function ContainmentApproveControl({
  plan,
  operationId,
  emphasis,
}: {
  emphasis?: "primary" | "secondary";
  operationId: string;
  plan: ApprovalPlan;
}) {
  const detail = useOperationStep(operationId, plan.id);
  return (
    <QueryBoundary
      query={detail}
      context="load the block's details"
      loading={<Skeleton shape="rows" />}
    >
      {(loaded) =>
        loaded.containment?.assurance === "unverified" ? (
          <UnverifiedApproveControl
            plan={plan}
            operationId={operationId}
            step={loaded.containment}
          />
        ) : (
          <StepApproveControl
            plan={plan}
            operationId={operationId}
            emphasis={emphasis ?? "primary"}
          />
        )
      }
    </QueryBoundary>
  );
}
