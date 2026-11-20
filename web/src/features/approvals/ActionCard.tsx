import type { Action } from "../../api/types";
import { ApprovalCard } from "./ApprovalCard";

export { EXPIRY_WARNING_MS, Expiry, suggestedBy } from "./approvalBits";

/**
 * One legacy action as read-only history, the same on the incident page, the Approvals page and in a chat. It maps
 * the action onto the shared `ApprovalCard`, which says what it shows.
 */
export function ActionCard({
  action,
  host,
  why,
  showIncident = false,
  incidentTitle,
}: {
  action: Action;
  /** The host's name; the card says "on nas" when it is known. */
  host?: string | undefined;
  /** The incident's title, as the text of the link to it; "View incident" without it. */
  incidentTitle?: string | undefined;
  /** Links to the incident: for lists that are not the incident's own page. */
  showIncident?: boolean;
  /** Why the model wants it, where the page has the report that says so. */
  why?: string | undefined;
}) {
  return (
    <ApprovalCard
      source={{ kind: "legacy", action }}
      host={host}
      why={why}
      {...(showIncident
        ? {
            incident: {
              id: action.incident_id,
              ...(incidentTitle !== undefined ? { title: incidentTitle } : {}),
            },
          }
        : {})}
    />
  );
}
