import { InlineMessage, Stack, Text, Visible } from "../../../components";
import { decisionErrorMeta } from "../../../design/tone";
import type { DecisionView } from "../decisionModel";
import { ConstraintsBlock } from "./ConstraintsBlock";
import { FactsBlock, MatchedBlock } from "./DetailBlocks";
import { LimitsBlock } from "./LimitsBlock";
import { ExceptionBlock, LockedBlock } from "./LockedBlock";
import { OutcomeBlock } from "./OutcomeBlock";
import { WhyBlock } from "./WhyBlock";

/**
 * One decision explained in plain words, in blocks ordered by how much they matter. It reads
 * a `DecisionView` from `decisionOf` and never raw JSON, so the simulator and the decision
 * page share it. Nothing here offers an approval.
 */
export function DecisionExplanation({
  title,
  decision,
  zone,
  emptyText,
}: {
  decision: DecisionView | null;
  /** Said in place of the matched rules when none matched. */
  emptyText?: string;
  /** Names the group for assistive technology. */
  title: string;
  /** The hub's time zone, for time windows. */
  zone: string | undefined;
}) {
  if (decision === null) return null;
  if (decision.kind === "error") {
    const meta = decisionErrorMeta(decision.code);
    return (
      <div role="group" aria-label={title}>
        <Visible category="P0" id="decision.outcome">
          <InlineMessage tone="warning" title={meta.label}>
            {meta.hint}
          </InlineMessage>
        </Visible>
      </div>
    );
  }
  const doc = decision.doc;
  return (
    <div role="group" aria-label={title}>
      <Stack gap={3}>
        <Visible category="P0" id="decision.outcome">
          <OutcomeBlock doc={doc} />
        </Visible>
        <Visible category="P0" id="decision.locked">
          <LockedBlock doc={doc} />
        </Visible>
        <Visible category="P0" id="decision.exception">
          <ExceptionBlock doc={doc} />
        </Visible>
        <Visible category="P0" id="decision.why">
          <WhyBlock doc={doc} />
        </Visible>
        <Visible category="P0" id="decision.limits">
          <LimitsBlock doc={doc} />
        </Visible>
        <Visible category="P1" id="decision.matched">
          {doc.matched.length === 0 && emptyText ? (
            <Text block muted>
              {emptyText}
            </Text>
          ) : (
            <MatchedBlock doc={doc} />
          )}
        </Visible>
        <Visible category="P1" id="decision.constraints">
          <ConstraintsBlock doc={doc} zone={zone} />
        </Visible>
        <Visible category="P2" id="decision.facts" label="Facts consulted" count={doc.facts.length}>
          <FactsBlock doc={doc} />
        </Visible>
      </Stack>
    </div>
  );
}
