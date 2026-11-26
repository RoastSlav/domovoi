import type { IncidentSummary } from "../../api/types";
import { verdictMeta } from "../../design/tone";

/**
 * What triage concluded, as a phrase of its own, for an incident whose status pill does not
 * already say it: one a person took, or one that resolved. Nothing if triage never ran.
 */
export function triageSaid(i: Pick<IncidentSummary, "status" | "verdict">): string | null {
  return i.status !== "open" && i.verdict ? `Triage said ${verdictMeta(i.verdict).label}` : null;
}
