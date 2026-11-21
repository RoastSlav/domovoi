import { Stack, StatusPill, Text } from "../../../components";
import { decisionReasonMeta } from "../../../design/tone";
import type { DecisionDoc } from "../decisionModel";
import { OutcomePill } from "../RuleMarks";

/** What a decision came to: the outcome, why in a word, and what follows from it. */
export function OutcomeBlock({ doc }: { doc: DecisionDoc }) {
  return (
    <Stack gap={2}>
      <div>
        <OutcomePill outcome={doc.outcome} /> <StatusPill meta={decisionReasonMeta(doc.reason)} />
      </div>
      {doc.outcome === "deny" && (
        <Text block muted>
          Not offered for approval. To allow it, change the policy.
        </Text>
      )}
      {doc.outcome === "unavailable" && (
        <Text block muted>
          Cannot decide. Nothing runs and nothing is offered.
        </Text>
      )}
    </Stack>
  );
}
