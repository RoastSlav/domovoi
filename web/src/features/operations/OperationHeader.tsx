import { usePauseOperation } from "../../api/hooks/operationControls";
import type { OperationSummary } from "../../api/types";
import { Button, Menu, PageHeader, StatusPill, Tag, useActionFeedback } from "../../components";
import { operationStatusMeta } from "../../design/tone";

const TERMINAL = new Set(["succeeded", "failed", "escalated", "cancelled"]);

/** True when the operation reached an end and no control applies. */
export function isTerminal(status: string): boolean {
  return TERMINAL.has(status);
}

/** Goal, status, cycle and hosts, with the Pause button and a menu holding Cancel. */
export function OperationHeader({
  operation,
  hostNames,
  onCancel,
}: {
  operation: OperationSummary;
  hostNames: Map<string, string>;
  onCancel: () => void;
}) {
  const pause = useActionFeedback(usePauseOperation(operation.id), {
    success: "Operation paused. Nothing new starts until you resume it.",
    error: "pause the operation",
  });
  const terminal = isTerminal(operation.status);
  const canPause = !terminal && operation.status !== "paused";
  return (
    <PageHeader
      title={operation.goal}
      meta={
        <>
          <StatusPill meta={operationStatusMeta(operation.status)} />
          {operation.cycle > 0 && <Tag>Cycle {operation.cycle}</Tag>}
          {operation.hosts.map((host) => (
            <Tag key={host}>{hostNames.get(host) ?? host}</Tag>
          ))}
        </>
      }
      actions={
        terminal ? undefined : (
          <>
            {canPause && (
              <Button
                icon="pause"
                loading={pause.isPending}
                disabledReason={
                  operation.status === "queued"
                    ? "It has not started yet. Cancel it instead."
                    : undefined
                }
                onClick={() => pause.mutate(undefined)}
              >
                Pause
              </Button>
            )}
            <Menu
              label="More"
              text="More"
              items={[{ label: "Cancel operation…", icon: "ban", onSelect: onCancel }]}
            />
          </>
        )
      }
    />
  );
}
