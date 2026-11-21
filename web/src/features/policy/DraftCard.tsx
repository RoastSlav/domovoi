import { Link } from "react-router";
import type { PolicyOverview } from "../../api/types";
import { Card, InlineMessage, RelativeTime, Stack, Text, Visible } from "../../components";
import { actorText } from "./ruleText";

/**
 * The warning shown when the active version moved on since the draft began.
 * @param overview The policy overview from the hub.
 */
export function DraftStale({ overview }: { overview: PolicyOverview }) {
  const { draft, active } = overview;
  if (!draft || (draft.based_on ?? null) === (active?.number ?? null)) return null;
  return (
    <Visible category="P0" id="policy.draft-stale">
      <InlineMessage
        tone="warning"
        title="The active version changed since this draft began"
        action={<Link to="/policy/simulator?rules=draft">Simulate the draft</Link>}
      >
        Review the draft before you publish. Saving it again re-bases it.
      </InlineMessage>
    </Visible>
  );
}

/**
 * The draft in progress: its revision, the version it began from and its locked rules.
 * @param overview The policy overview from the hub.
 */
export function DraftCard({ overview }: { overview: PolicyOverview }) {
  const { draft } = overview;
  if (!draft) return null;
  return (
    <Visible category="P1" id="policy.draft">
      <Card label="Draft">
        <Stack gap={2}>
          <Text strong block>
            Draft
          </Text>
          <Link to="/policy/draft">Revision {draft.revision}</Link>
          <Text block>
            {draft.based_on != null ? `Based on version ${draft.based_on}` : "Started empty"}
          </Text>
          <Text muted size="sm" block>
            {draft.rule_count} rules, {draft.locked_count} locked. Last changed by{" "}
            {actorText(draft.updated_by)} <RelativeTime at={draft.updated_at} />.
          </Text>
        </Stack>
      </Card>
    </Visible>
  );
}
