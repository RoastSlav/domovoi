import { Link } from "react-router";
import type { PolicyOverview } from "../../api/types";
import { InlineMessage, RelativeTime, Stack, Text, Visible } from "../../components";
import { actorText } from "./ruleText";

/**
 * The draft in progress: its revision, the version it began from and its locked rules, plus a
 * warning when the active version moved on since it began.
 * @param overview The policy overview from the hub.
 */
export function DraftCard({ overview }: { overview: PolicyOverview }) {
  const { draft, active } = overview;
  if (!draft) return null;
  const stale = (draft.based_on ?? null) !== (active?.number ?? null);
  return (
    <Stack gap={3}>
      {stale && (
        <Visible category="P0" id="policy.draft-stale">
          <InlineMessage
            tone="warning"
            title="The active version changed since this draft began"
            action={<Link to="/policy/simulator?rules=draft">Simulate the draft</Link>}
          >
            Review the draft before you publish. Saving it again re-bases it.
          </InlineMessage>
        </Visible>
      )}
      <Visible category="P1" id="policy.draft">
        <Stack gap={1}>
          <Link to="/policy/draft">Draft revision {draft.revision}</Link>
          <Text>
            {draft.based_on != null ? `Based on version ${draft.based_on}` : "Started empty"}
          </Text>
          <Text muted size="sm">
            {draft.rule_count} rules, {draft.locked_count} locked. Last changed by{" "}
            {actorText(draft.updated_by)} <RelativeTime at={draft.updated_at} />.
          </Text>
        </Stack>
      </Visible>
    </Stack>
  );
}
