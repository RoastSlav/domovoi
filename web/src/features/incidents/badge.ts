import type { IncidentSummary } from "../../api/types";
import { badgeLabel } from "../../components/Badge";

/** The badge an incident shows: Resolved once it resolved, else its verdict or severity. */
export function incidentBadge(i: Pick<IncidentSummary, "status" | "verdict" | "severity">): string {
  return i.status === "resolved" ? "resolved" : (i.verdict ?? i.severity);
}

/** What triage concluded about a resolved incident, or nothing if it never ran. */
export function triageNote(i: Pick<IncidentSummary, "status" | "verdict">): string | null {
  return i.status === "resolved" && i.verdict ? `triage said ${badgeLabel(i.verdict)}` : null;
}
