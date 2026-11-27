import type { IncidentDetail } from "../../api/types";
import { InlineMessage } from "../../components";

/** Security events that came after the report below: its verdict says nothing about them. */
export function UncoveredNotice({
  uncovered,
}: {
  uncovered: NonNullable<IncidentDetail["uncovered"]>;
}) {
  const { events, latest } = uncovered;
  const what = events === 1 ? "1 later security event" : `${events} later security events`;
  return (
    <InlineMessage tone="warning" title={`This verdict does not cover ${what}`}>
      Latest: {latest}. Each one is on the timeline. Run triage again to have them looked at.
    </InlineMessage>
  );
}
