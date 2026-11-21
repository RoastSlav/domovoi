import { RetentionKept } from "../views/RetentionKept";
import { fixed, type SectionSpec } from "./types";

/** The label of each `[retention]` period, in the hub's order; an unknown key shows its raw name. */
export const RETENTION_LABELS: Readonly<Record<string, string>> = {
  signals_days: "Signals",
  incident_timeline_days: "Incident timeline",
  conversations_days: "Follow-up conversations",
  notifications_days: "Notification state",
  triage_cache_days: "Cached triage reports",
  resolved_advisories_days: "Resolved advisories",
  operation_journal_days: "Operation journal",
  execution_outputs_days: "Raw action output",
  retracted_facts_days: "Retracted knowledge",
};

/** How long raw data is kept; changing a period asks for the password. */
export const RETENTION_SECTION: SectionSpec = {
  id: "retention",
  title: "Retention",
  intro:
    "How long raw data is kept, from 1 to 3650 days. Operations, their summaries and the audit log are kept and have no period.",
  view: RetentionKept,
  fields: fixed(
    Object.entries(RETENTION_LABELS).map(([key, label]) => ({
      path: `retention.${key}`,
      label: `${label} (days)`,
      kind: "number" as const,
    })),
  ),
};
