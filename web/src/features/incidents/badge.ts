import type { IncidentSummary } from "../../api/types";

/** The badge an incident shows: Resolved once it resolved, else its verdict or severity. */
export function incidentBadge(i: Pick<IncidentSummary, "status" | "verdict" | "severity">): string {
  return i.status === "resolved" ? "resolved" : (i.verdict ?? i.severity);
}
