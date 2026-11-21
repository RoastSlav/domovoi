import { Card, InlineMessage, Stack, StatusPill, Text } from "../../../components";
import { ruleMarkMeta } from "../../../design/tone";
import type { DecisionDoc } from "../decisionModel";
import { OutcomePill } from "../RuleMarks";

/** The locked rules that decided, and what the rules alone would have said. */
export function LockedBlock({ doc }: { doc: DecisionDoc }) {
  if (doc.locked.length === 0) return null;
  const would = doc.wouldHaveBeen;
  return (
    <Card tone="danger">
      <Stack gap={2}>
        <div>
          <StatusPill meta={ruleMarkMeta("locked_deny")} />
        </div>
        <Text block>Locked by {doc.locked.join(", ")}</Text>
        {would && (
          <Text block>
            Without the lock, the rules would have decided <OutcomePill outcome={would.outcome} />
          </Text>
        )}
      </Stack>
    </Card>
  );
}

/** One warning per Allow that overrides a broader Deny. */
export function ExceptionBlock({ doc }: { doc: DecisionDoc }) {
  const found = doc.warnings.flatMap((w) => (w.kind === "dangerous_exception" ? [w] : []));
  const title = ruleMarkMeta("dangerous_exception").label;
  return (
    <>
      {found.map((w) => (
        <InlineMessage key={`${w.allow}>${w.deny}`} tone="warning" title={title}>
          Rule {w.allow} allows this and overrides the broader Deny {w.deny}.
        </InlineMessage>
      ))}
    </>
  );
}
