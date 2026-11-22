import { useOperationStep } from "../../api/hooks/operations";
import type { OperationTrace } from "../../api/operationTrace";
import type { Execution } from "../../api/types";
import { RightSlotSurface } from "../../app/rightSlot";
import { InlineMessage, Visible } from "../../components";
import { stepKindMeta } from "../../design/tone";
import { useReplayStatus } from "./replay/ReplayContext";
import { inspectorNote } from "./replay/replayText";
import { INSPECTOR_ID } from "./useStepSelection";
import {
  StepAction,
  StepConnections,
  StepDetails,
  StepFacts,
  StepReason,
} from "./InspectorSections";

/**
 * The step inspector, a surface in the right-hand slot. It opens while a step is selected
 * (the page owns the selection) and shows the step, its action card read-only, its
 * connections and its stored intent and output.
 */
export function StepInspector({
  operationId,
  trace,
  executions,
  hostNames,
  selected,
  onSelect,
  onClose,
  onReview,
}: {
  operationId: string;
  trace: OperationTrace;
  executions: readonly Execution[];
  hostNames: Map<string, string>;
  selected: string | undefined;
  onSelect: (id: string, opener: HTMLElement) => void;
  onClose: () => void;
  onReview: () => void;
}) {
  const step = trace.steps.find((candidate) => candidate.id === selected);
  const execution = executions.find((candidate) => candidate.step === selected);
  const replay = useReplayStatus();
  return (
    <RightSlotSurface
      spec={{ id: INSPECTOR_ID, label: "Step inspector", icon: "search" }}
      onClose={onClose}
    >
      {step && (
        <>
          {replay.replaying && (
            <Visible category="P0" id="inspector.replay-note">
              <InlineMessage tone="info" title="Showing the live record">
                {inspectorNote(replay.label)}
              </InlineMessage>
            </Visible>
          )}
          <p className="visually-hidden" role="status">
            Showing {stepKindMeta(step.kind).label}
            {step.summary ? `: ${step.summary}` : ""}
          </p>
          <StepFacts step={step} />
          <StepReasonLoader operationId={operationId} stepId={step.id} outcome={step.outcome} />
          {step.kind === "execution" && (
            <StepAction
              operationId={operationId}
              execution={execution}
              hostNames={hostNames}
              onReview={onReview}
            />
          )}
          <StepConnections trace={trace} stepId={step.id} onSelect={onSelect} />
          <StepDetails operationId={operationId} stepId={step.id} />
        </>
      )}
    </RightSlotSurface>
  );
}

function StepReasonLoader({
  operationId,
  stepId,
  outcome,
}: {
  operationId: string;
  stepId: string;
  outcome: string;
}) {
  const detail = useOperationStep(operationId, stepId);
  return <StepReason reason={detail.data?.reason ?? null} outcome={outcome} />;
}
