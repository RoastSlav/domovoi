import { useState } from "react";
import { useAnswerStep } from "../../api/hooks/operationControls";
import { useOperationStep } from "../../api/hooks/operations";
import type { TraceStep } from "../../api/operationTrace";
import {
  Button,
  Card,
  Cluster,
  Field,
  Stack,
  Text,
  Textarea,
  useActionFeedback,
} from "../../components";
import { Expiry } from "../approvals/approvalBits";

const MAX_ANSWER = 2000;

/** What the stored intent of a question step holds, as far as this card reads it. */
function intentOf(intent: unknown): { text?: string; expires?: string } {
  if (typeof intent !== "object" || intent === null) {
    return {};
  }
  const record = intent as Record<string, unknown>;
  return {
    ...(typeof record.text === "string" ? { text: record.text } : {}),
    ...(typeof record.expires_at === "string" ? { expires: record.expires_at } : {}),
  };
}

/**
 * A question Domovoi asked and cannot answer itself, with a field for the answer. The
 * question is model text: shown as plain text. The draft lives only while the card does.
 */
export function QuestionCard({
  operationId,
  step,
  paused,
}: {
  operationId: string;
  step: Pick<TraceStep, "id" | "summary">;
  paused: boolean;
}) {
  const detail = useOperationStep(operationId, step.id);
  const [draft, setDraft] = useState("");
  const answer = useActionFeedback(useAnswerStep(operationId), {
    success: "Answer sent.",
    error: "send the answer",
  });
  const { text, expires } = intentOf(detail.data?.intent);
  const reason = paused
    ? "Resume the operation before answering."
    : draft.trim() === ""
      ? "Write an answer first."
      : draft.length > MAX_ANSWER
        ? `The answer is over ${MAX_ANSWER} characters. Shorten it.`
        : undefined;
  return (
    <Card tone="pending" label="Question">
      <Stack gap={3}>
        <Text strong block>
          {text ?? step.summary ?? "Domovoi asked a question."}
        </Text>
        {expires && <Expiry until={expires} />}
        <Field
          label="Your answer"
          hint={`Up to ${MAX_ANSWER} characters. It goes to the model as your answer.`}
        >
          <Textarea value={draft} rows={3} onChange={(event) => setDraft(event.target.value)} />
        </Field>
        <Cluster>
          <Button
            variant="primary"
            loading={answer.isPending}
            disabledReason={reason}
            onClick={() =>
              answer.mutate({ step: step.id, text: draft }, { onSuccess: () => setDraft("") })
            }
          >
            Send answer
          </Button>
        </Cluster>
      </Stack>
    </Card>
  );
}
