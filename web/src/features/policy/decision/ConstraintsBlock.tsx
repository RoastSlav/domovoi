import { Stack, Text } from "../../../components";
import type { DecisionDoc } from "../decisionModel";
import { conditionViewText, durationText } from "../ruleText";

/** The limits an automatic Allow carries. */
export function ConstraintsBlock({ doc, zone }: { doc: DecisionDoc; zone: string | undefined }) {
  const c = doc.constraints;
  const lines = [
    ...(c.maxPerHour !== null ? [`At most ${c.maxPerHour} per hour`] : []),
    ...(c.maxTtlSeconds !== null ? [`Lasts at most ${durationText(c.maxTtlSeconds)}`] : []),
    ...(c.targets !== null ? [`Only for targets: ${c.targets.join(", ")}`] : []),
    ...c.conditions.map((x) => conditionViewText(x, zone)),
  ];
  if (doc.outcome !== "allow" || lines.length === 0) return null;
  return (
    <Stack gap={1}>
      {lines.map((line, index) => (
        // The same sentence can appear twice (a limit and a condition), so the place is part of the key.
        <Text key={`${index}:${line}`} block>
          {line}
        </Text>
      ))}
      <Text block muted>
        An automatic Allow carries these limits.
      </Text>
    </Stack>
  );
}
