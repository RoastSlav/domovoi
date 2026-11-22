import type { Action, OperationSummary } from "../../api/types";
import { useOperation } from "../../api/hooks/operations";
import { Button, InlineMessage, Skeleton } from "../../components";
import { ActionCard } from "../approvals/ActionCard";
import { ApprovalCard } from "../approvals/ApprovalCard";
import { awaitingExecutions, planTextFor } from "../operations/steps";
import styles from "./attention.module.css";

/** The cards of one operation's steps that wait for approval; failure and loading are visible. */
function StepApprovals({
  operation,
  hostNames,
  emphasis,
}: {
  operation: OperationSummary;
  hostNames: ReadonlyMap<string, string>;
  emphasis: "primary" | "secondary";
}) {
  const query = useOperation(operation.id);
  if (query.isError && !query.data) {
    return (
      <InlineMessage
        tone="warning"
        title={`Could not load ${operation.goal}`}
        action={
          <Button variant="secondary" onClick={() => void query.refetch()}>
            Retry
          </Button>
        }
      />
    );
  }
  if (!query.data) {
    return <Skeleton shape="block" />;
  }
  const detail = query.data;
  return (
    <>
      {awaitingExecutions(detail.executions).map((execution) => (
        <ApprovalCard
          key={execution.step}
          source={{ kind: "step", operationId: operation.id, execution }}
          host={hostNames.get(execution.host_id)}
          why={planTextFor(detail.trace, execution.step)}
          emphasis={emphasis}
          operation={{ id: operation.id, goal: operation.goal }}
        />
      ))}
    </>
  );
}

/**
 * The approval cards for operations (the hub's order) and then classic actions (soonest
 * expiry first). It decides nothing itself: Approve and Deny live inside the cards.
 */
export function ApprovalGroup({
  operations,
  actions,
  hostNames,
  limit,
  emphasis,
}: {
  operations: readonly OperationSummary[];
  actions: readonly Action[];
  hostNames: ReadonlyMap<string, string>;
  limit?: number | undefined;
  emphasis: "primary" | "secondary";
}) {
  const byExpiry = [...actions].sort((a, b) => Date.parse(a.expires_at) - Date.parse(b.expires_at));
  const items = [
    ...operations.map((operation) => ({ operation }) as const),
    ...byExpiry.map((action) => ({ action }) as const),
  ].slice(0, limit);
  return (
    <div className={styles.cards}>
      {items.map((entry) =>
        "operation" in entry ? (
          <StepApprovals
            key={entry.operation.id}
            operation={entry.operation}
            hostNames={hostNames}
            emphasis={emphasis}
          />
        ) : (
          <ActionCard
            key={entry.action.id}
            action={entry.action}
            host={hostNames.get(entry.action.host_id)}
            showIncident
          />
        ),
      )}
    </div>
  );
}
