import { useState } from "react";
import { useResumeOperation } from "../../api/hooks/operationControls";
import type { OperationTrace, TraceStep } from "../../api/operationTrace";
import type { Execution, OperationSummary } from "../../api/types";
import { Banner, Button, ConfirmDialog, useActionFeedback } from "../../components";
import type { BudgetKind } from "../../design/operationText";
import { operationBanner, type BannerAction } from "./banner";
import { RaiseLimitDialog } from "./RaiseLimitDialog";
import styles from "./operations.module.css";

const LABELS: Record<BannerAction, string> = {
  show_step: "Show the step",
  resume: "Resume operation",
  raise_limit: "Raise limit and resume",
  review: "Review",
  cancel: "Cancel operation",
};

/**
 * The one banner for the state that needs attention, with the buttons that act on it. Resume
 * after an unknown result asks first; a manual or error pause resumes at once; a spent budget
 * opens the raise-limit dialog.
 */
export function OperationBanner({
  operation,
  trace,
  executions,
  hostNames,
  onShowStep,
  onReview,
  onCancel,
}: {
  operation: OperationSummary;
  trace: OperationTrace;
  executions: readonly Execution[];
  hostNames: Map<string, string>;
  onShowStep: (step: TraceStep) => void;
  onReview: () => void;
  onCancel: () => void;
}) {
  const [dialog, setDialog] = useState<"resume" | "raise" | null>(null);
  const resume = useActionFeedback(useResumeOperation(operation.id), {
    success: "Operation resumed",
    error: "resume the operation",
    errorDisplay: "inline",
  });
  const spec = operationBanner(operation, trace.steps, executions, hostNames);
  if (!spec) {
    return null;
  }
  const target = spec.step ? trace.steps.find((step) => step.id === spec.step) : undefined;
  const exhausted =
    operation.pause_reason?.type === "budget" ? (operation.pause_reason.exhausted ?? "") : "";

  const run = (action: BannerAction) => {
    switch (action) {
      case "show_step":
        if (target) {
          onShowStep(target);
        }
        return;
      case "resume":
        if (spec.key === "outcome_unknown") {
          setDialog("resume");
        } else {
          resume.mutate({ extend: [] });
        }
        return;
      case "raise_limit":
        setDialog("raise");
        return;
      case "review":
        onReview();
        return;
      case "cancel":
        onCancel();
    }
  };
  const actions = spec.actions.filter((action) => action !== "show_step" || target);
  const where = trace.steps.find((step) => step.id === spec.step)?.summary ?? "the action";
  return (
    <>
      <Banner
        tone={spec.tone}
        title={spec.title}
        action={
          actions.length > 0 ? (
            <div className={styles.bannerActions}>
              {actions.map((action) => (
                <Button key={action} size="sm" onClick={() => run(action)}>
                  {LABELS[action]}
                </Button>
              ))}
            </div>
          ) : undefined
        }
      >
        {spec.body}
      </Banner>
      {dialog === "resume" && (
        <ConfirmDialog
          title="Continue without knowing the result?"
          consequence={`Domovoi does not know whether ${where} ran on the host. Continuing may run it again.`}
          confirmLabel="Resume operation"
          busy={resume.isPending}
          error={resume.error}
          onCancel={() => {
            setDialog(null);
            resume.reset();
          }}
          onConfirm={() => resume.mutate({ extend: [] }, { onSuccess: () => setDialog(null) })}
        />
      )}
      {dialog === "raise" && (
        <RaiseLimitDialog
          operationId={operation.id}
          trace={trace}
          kind={exhausted as BudgetKind}
          onClose={() => setDialog(null)}
        />
      )}
    </>
  );
}
