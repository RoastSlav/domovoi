import { useState } from "react";
import { useResumeOperation } from "../../api/hooks/operationControls";
import type { OperationTrace } from "../../api/operationTrace";
import {
  Button,
  Dialog,
  ErrorDetail,
  Field,
  InlineMessage,
  NumberInput,
  Text,
  useActionFeedback,
} from "../../components";
import { budgetLabel, type BudgetKind } from "../../design/operationText";
import { formatBudget, fromApiLimit, limitFor, toApiLimit } from "./budget";

/** The suggested new limit, in display units: half as much again, and always above the old one. */
function suggestion(limit: number): number {
  return Math.max(limit + 1, Math.ceil(limit * 1.5));
}

/**
 * Raises the budget that ran out and resumes the operation. The hub refuses a lower limit;
 * the dialog checks first and shows the hub's answer inside itself if it refuses anyway.
 */
export function RaiseLimitDialog({
  operationId,
  trace,
  kind,
  onClose,
}: {
  operationId: string;
  trace: OperationTrace;
  kind: BudgetKind;
  onClose: () => void;
}) {
  const { limit, used } = limitFor(kind, trace);
  const current = limit === null ? 0 : fromApiLimit(kind, limit);
  const [value, setValue] = useState(String(limit === null ? 1 : suggestion(current)));
  const [invalid, setInvalid] = useState(false);
  const resume = useActionFeedback(useResumeOperation(operationId), {
    success: "Limit raised. The operation resumed.",
    error: "raise the limit",
    errorDisplay: "inline",
  });
  const label = budgetLabel(kind);
  const nowText =
    limit === null ? "none" : formatBudget(kind, limit, null).replace(" (no limit)", "");
  const submit = () => {
    const next = Number(value);
    if (!Number.isFinite(next) || value.trim() === "" || (limit !== null && next <= current)) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    resume.mutate({ extend: [{ kind, limit: toApiLimit(kind, next) }] }, { onSuccess: onClose });
  };
  return (
    <Dialog
      title={`Raise the ${label} limit`}
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" loading={resume.isPending} onClick={submit}>
            Raise limit and resume
          </Button>
        </>
      }
    >
      <Text block>
        Limit now: {nowText}. Used: {formatBudget(kind, used, null).replace(" (no limit)", "")}.
      </Text>
      <Field
        label={`New ${label.toLowerCase()} limit`}
        hint={`Enter a limit above ${nowText}.`}
        error={invalid ? `Enter a limit above ${nowText}.` : undefined}
      >
        <NumberInput value={value} onChange={(event) => setValue(event.target.value)} />
      </Field>
      {resume.error && (
        <InlineMessage tone="danger" title={resume.error.title}>
          <ErrorDetail error={resume.error} />
        </InlineMessage>
      )}
    </Dialog>
  );
}
