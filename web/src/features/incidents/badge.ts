import type { IncidentSummary } from "../../api/types";
import { verdictMeta } from "../../design/tone";

/** What triage concluded about a resolved incident, or nothing if it never ran. */
export function triageNote(i: Pick<IncidentSummary, "status" | "verdict">): string | null {
  return i.status === "resolved" && i.verdict
    ? `triage said ${verdictMeta(i.verdict).label}`
    : null;
}
