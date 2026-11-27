import type { IncidentDetail } from "../../api/types";
import { InlineMessage, Text } from "../../components";

/**
 * A check that cannot read what it watches, where triage's report would be: why, and what to
 * do on the host. The hub words both and runs no triage by itself, so there is none to wait for.
 */
export function BlindNotice({
  blind,
  resolved,
}: {
  blind: NonNullable<IncidentDetail["blind"]>;
  resolved: boolean;
}) {
  return (
    <InlineMessage
      tone="warning"
      title={
        resolved
          ? "This check could not see what it watches"
          : "This check cannot see what it watches"
      }
    >
      <Text block>
        <Text muted>Why: </Text>
        {blind.why}
      </Text>
      {!resolved && (
        <Text block>
          <Text muted>Fix: </Text>
          {blind.fix}
        </Text>
      )}
    </InlineMessage>
  );
}
