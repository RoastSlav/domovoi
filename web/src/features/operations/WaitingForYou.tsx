import type { OperationTrace } from "../../api/operationTrace";
import type { Execution } from "../../api/types";
import { Section, Stack } from "../../components";
import { ApprovalCard } from "../approvals/ApprovalCard";
import { QuestionCard } from "./QuestionCard";
import { awaitingExecutions, pendingQuestions, planTextFor } from "./steps";

/** Moves focus to the "Waiting for you" heading and scrolls it into view. */
export function focusWaiting(): void {
  const heading = document.getElementById("waiting-for-you")?.querySelector("h2");
  if (heading) {
    heading.tabIndex = -1;
    heading.focus();
    heading.scrollIntoView?.({ block: "start" });
  }
}

/** Everything that waits for a person: one approval card per open action, then the questions. */
export function WaitingForYou({
  operationId,
  trace,
  executions,
  hostNames,
  paused,
}: {
  operationId: string;
  trace: OperationTrace;
  executions: readonly Execution[];
  hostNames: Map<string, string>;
  paused: boolean;
}) {
  const approvals = awaitingExecutions(executions);
  const questions = pendingQuestions(trace);
  if (approvals.length + questions.length === 0) {
    return null;
  }
  return (
    <div id="waiting-for-you">
      <Section title="Waiting for you">
        <Stack gap={3}>
          {approvals.map((execution) => (
            <ApprovalCard
              key={execution.step}
              source={{ kind: "step", operationId, execution }}
              host={hostNames.get(execution.host_id)}
              why={planTextFor(trace, execution.step)}
              emphasis={approvals.length === 1 ? "primary" : "secondary"}
            />
          ))}
          {questions.map((step) => (
            <QuestionCard key={step.id} operationId={operationId} step={step} paused={paused} />
          ))}
        </Stack>
      </Section>
    </div>
  );
}
