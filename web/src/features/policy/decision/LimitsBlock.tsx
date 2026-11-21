import { Card, InfoTip, InlineMessage, Stack, StatusPill, Text } from "../../../components";
import { hardLimitMeta } from "../../../design/tone";
import type { DecisionDoc } from "../decisionModel";
import { OutcomePill } from "../RuleMarks";

/** The hard limits that applied after the rules, apart from the rules themselves. */
export function LimitsBlock({ doc }: { doc: DecisionDoc }) {
  const unverified = doc.warnings.some((w) => w.kind === "safety_set_unverified");
  const paths = doc.warnings.flatMap((w) =>
    w.kind === "management_paths_unprotected" ? w.paths : [],
  );
  const unprotected = doc.warnings.some((w) => w.kind === "management_paths_unprotected");
  const would = doc.wouldHaveBeen;
  if (doc.limits.length === 0 && !unverified && !unprotected && !would && !doc.typedConfirmation) {
    return null;
  }
  return (
    <Card>
      <Stack gap={3}>
        <Text strong>
          Hard limits
          <InfoTip title="Hard limits">
            Hard limits apply after the rules. They are not rules and cannot be edited here.
          </InfoTip>
        </Text>
        {doc.limits.map((limit) => (
          <div key={limit}>
            <StatusPill meta={hardLimitMeta(limit)} />
            <Text block muted>
              {hardLimitMeta(limit).hint}
            </Text>
          </div>
        ))}
        {unverified && (
          <InlineMessage tone="warning">
            The set of protected paths could not be verified.
          </InlineMessage>
        )}
        {unprotected && (
          <InlineMessage tone="warning">Not protected: {paths.join(", ")}</InlineMessage>
        )}
        {would && (
          <Text block>
            Rules alone: <OutcomePill outcome={would.outcome} />. After the limits:{" "}
            <OutcomePill outcome={doc.outcome} />.
          </Text>
        )}
        {doc.typedConfirmation && (
          <Text block>Approving this would need a typed confirmation.</Text>
        )}
      </Stack>
    </Card>
  );
}
