import { Link } from "react-router";
import type { IncidentSummary } from "../../api/types";
import { RelativeTime, StatusPill, Text, type Column } from "../../components";
import { incidentStateMeta, severityMeta } from "../../design/tone";
import { triageSaid } from "./badge";
import { IncidentRowMenu } from "./IncidentRowMenu";

export function checkText(incident: IncidentSummary): string {
  return incident.target ? `${incident.check} ${incident.target}` : incident.check;
}

/** What an empty Triage cell says: triage has not given a view on an incident in this state. */
const NO_TRIAGE_NOTE =
  "Nothing to add: the status already says what triage concluded, or triage has not looked at it.";

function Triage({ incident }: { incident: IncidentSummary }) {
  return triageSaid(incident) ?? "–";
}

/** Which optional columns a table has. */
export interface ColumnChoice {
  /** The group says the status already (Action needed: every row is open). */
  hideStatus: boolean;
  /** Some row has a triage view to show. */
  showTriage: boolean;
}

/** Whether any of `incidents` has something to say in the Triage column. */
export function hasTriage(incidents: readonly IncidentSummary[]): boolean {
  return incidents.some((incident) => triageSaid(incident));
}

/**
 * The table's columns. Every width is fixed so the groups of one page line up; the title
 * takes what is left and clamps to two lines, and the long mono check and the host stay on
 * one line, with their full text as a tooltip.
 */
export function incidentColumns(
  hostNames: Map<string, string>,
  { hideStatus, showTriage }: ColumnChoice,
): Column<IncidentSummary>[] {
  const state: Column<IncidentSummary>[] = hideStatus
    ? []
    : [
        {
          key: "state",
          label: "Status",
          // Wide enough for the longest state pill, "Acknowledged".
          width: "9rem",
          cell: (incident) => <StatusPill meta={incidentStateMeta(incident)} />,
        },
      ];
  const triage: Column<IncidentSummary>[] = showTriage
    ? [
        {
          key: "triage",
          label: "Triage",
          width: "7rem",
          truncate: "ellipsis",
          tip: (incident) => triageSaid(incident) ?? NO_TRIAGE_NOTE,
          cell: (incident) => <Triage incident={incident} />,
        },
      ]
    : [];
  return [
    ...state,
    {
      key: "title",
      label: "Incident",
      truncate: "clamp2",
      tip: (incident) => incident.title,
      cell: (incident) => (
        <Link to={`/incidents/${incident.id}`}>
          <Text strong>{incident.title}</Text>
        </Link>
      ),
    },
    {
      key: "host",
      label: "Host",
      width: "6rem",
      truncate: "ellipsis",
      tip: (incident) => hostNames.get(incident.host_id) ?? "Unknown host",
      cell: (incident) => hostNames.get(incident.host_id) ?? "Unknown host",
    },
    {
      key: "check",
      label: "Check",
      width: "11rem",
      truncate: "ellipsis",
      tip: checkText,
      cell: (incident) => (
        <Text mono size="sm">
          {checkText(incident)}
        </Text>
      ),
    },
    {
      key: "severity",
      label: "Severity",
      width: "6rem",
      cell: (incident) => <StatusPill size="sm" meta={severityMeta(incident.severity)} />,
    },
    {
      key: "fired",
      label: "Last fired",
      width: "6.5rem",
      cell: (incident) => <RelativeTime at={incident.last_firing_at} />,
    },
    {
      key: "times",
      label: "Times",
      width: "3.5rem",
      numeric: true,
      hint: "How many times the check has fired for this incident.",
      cell: (incident) => incident.occurrences,
    },
    ...triage,
    {
      key: "actions",
      label: "Actions",
      width: "3rem",
      hideLabel: true,
      cell: (incident) => <IncidentRowMenu incident={incident} />,
    },
  ];
}
