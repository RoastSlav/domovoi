import { useIncident } from "../../api/hooks/incidents";
import type { Action } from "../../api/types";
import { ActionCard } from "./ActionCard";
import { whyProposed } from "./whyProposed";

/**
 * An action waiting on the Approvals page. The action alone does not say why the model
 * wants it or what it is for, so the card reads the incident (one request per incident,
 * shared by its actions) for the reason and the title. If that fails the card still shows
 * everything the action has: the reason is a nicety, not a gate.
 */
export function WaitingCard({ action, host }: { action: Action; host: string | undefined }) {
  const detail = useIncident(action.incident_id).data;
  return (
    <ActionCard
      action={action}
      host={host}
      showIncident
      incidentTitle={detail?.incident.title}
      why={whyProposed(action, detail?.report)}
    />
  );
}
