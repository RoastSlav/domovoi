import { useAdvisories } from "../../api/hooks/system";
import { InlineMessage, LinkButton, RelativeTime } from "../../components";

/** The kind of the advisory the triage circuit breaker raises. */
export const TRIAGE_DEGRADED = "triage_degraded";

/**
 * Says that triage cannot reach its model: one notice for the whole system, at the top of the
 * Control room and the Attention centre. Incidents that need triage wait for the model and
 * carry on by themselves; the notice goes when the model answers again.
 */
export function ModelNotice() {
  const advisories = useAdvisories();
  const degraded = advisories.data?.find((advisory) => advisory.kind === TRIAGE_DEGRADED);
  if (!degraded) {
    return null;
  }
  return (
    <InlineMessage
      tone="warning"
      title="Triage cannot reach its model"
      action={
        <LinkButton to="/settings/models" variant="secondary">
          Model settings
        </LinkButton>
      }
    >
      <span>
        {degraded.detail} Since <RelativeTime at={degraded.created_at} />. Incidents that need triage
        wait for the model and carry on when it answers; Run triage on an incident tries it by hand.
      </span>
    </InlineMessage>
  );
}
