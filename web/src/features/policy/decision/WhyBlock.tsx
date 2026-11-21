import { Cluster, Stack, Tag, Text } from "../../../components";
import { decisionReasonMeta } from "../../../design/tone";
import type { DecisionDoc } from "../decisionModel";

/** Why the decision came out as it did: the reason, the winning rules and what they overrode. */
export function WhyBlock({ doc }: { doc: DecisionDoc }) {
  return (
    <Stack gap={2}>
      <Text block>{decisionReasonMeta(doc.reason).hint}</Text>
      {doc.winners.length > 0 && (
        <Cluster gap={2}>
          {doc.winners.map((name) => (
            <Tag key={name}>{name}</Tag>
          ))}
        </Cluster>
      )}
      {doc.overridden.map((o) => (
        <Text key={`${o.by}>${o.broader}`} block muted>
          {o.by} overrides the broader rule {o.broader}
        </Text>
      ))}
    </Stack>
  );
}
