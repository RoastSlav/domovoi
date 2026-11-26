import { useState } from "react";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  EmptyState,
  PageHeader,
  Section,
  Skeleton,
  Stack,
  Visible,
  VisibilityPage,
} from "../../components";
import { TIER_TITLES, type Tier } from "./attentionModel";
import { ApprovalGroup } from "./ApprovalGroup";
import { AttentionRows } from "./AttentionRows";
import { SourceFailures, TruncatedNote } from "./SourceNotices";
import { useAttentionSources } from "./useAttentionSources";

/** How many approval cards show before "Show more". */
export const ATTENTION_CARDS = 10;

const TIERS: readonly Tier[] = [0, 1, 2];

/** Everything that waits for the owner, most urgent first, in three tiers. */
export function AttentionPage() {
  const [host] = useSelectedHost();
  const state = useAttentionSources(host);
  const [shown, setShown] = useState(ATTENTION_CARDS);
  const { inputs } = state;
  const hostNames = inputs.hostNames;
  const rows = state.items.filter((item) => item.kind !== "approval");
  const approvals = inputs.waitingApproval.length;
  const nothing = state.allLoaded && state.items.length === 0;
  return (
    <VisibilityPage page="attention">
      <PageHeader
        title="Attention"
        intro="What waits for you, most urgent first."
        docs="guide/control-room.html"
      />
      <Visible category="P0" id="attention.list">
        <Stack gap={6}>
          <SourceFailures failed={state.failed} />
          <TruncatedNote truncated={state.truncated} />
          {state.pending && state.items.length === 0 && <Skeleton shape="rows" count={5} />}
          {nothing && (
            <EmptyState icon="check-circle" title="Nothing needs you">
              <p>No result is unknown, nothing is paused and no host or incident needs action.</p>
            </EmptyState>
          )}
          {TIERS.map((tier) => {
            const tierRows = rows.filter((item) => item.tier === tier);
            const cards = tier === 1 && approvals > 0;
            if (tierRows.length === 0 && !cards) {
              return null;
            }
            return (
              <Section key={tier} title={TIER_TITLES[tier]}>
                {cards && (
                  <>
                    <ApprovalGroup
                      operations={inputs.waitingApproval}
                      hostNames={hostNames}
                      limit={shown}
                      emphasis="primary"
                    />
                    {approvals > shown && (
                      <Button variant="secondary" onClick={() => setShown(shown + ATTENTION_CARDS)}>
                        {`Show ${Math.min(ATTENTION_CARDS, approvals - shown)} more`}
                      </Button>
                    )}
                  </>
                )}
                {tierRows.length > 0 && <AttentionRows items={tierRows} hostNames={hostNames} />}
              </Section>
            );
          })}
        </Stack>
      </Visible>
    </VisibilityPage>
  );
}
