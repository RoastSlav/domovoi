import type { Host, IncidentSummary } from "../api/types";
import { ACTION_STATUSES, RISKS, TRIAGE } from "./actionVocab";
import {
  CHECK_STATES,
  HOST_CONNECTIONS,
  HOST_KINDS,
  ISSUE_SEVERITIES,
  LIVE,
  SSH_LOGINS,
  STEP_STATES,
} from "./fleetVocab";
import { INCIDENT_GROUPS, INCIDENT_STATUSES, SEVERITIES, VERDICTS } from "./respondVocab";
import { lookup, type HostConnection, type IncidentGroup, type StatusMeta } from "./status";

/**
 * The single place where a status becomes a label, a tone, a hint and an icon.
 * Components never decide a status's wording or colour themselves: they ask here. The
 * tables are in `respondVocab.ts`, `actionVocab.ts` and `fleetVocab.ts`, split only to keep
 * files short; import from this file, never from them.
 */

export { TONES } from "./status";
export type {
  ActionStatus,
  CheckState,
  HostConnection,
  HostKind,
  IncidentGroup,
  IncidentStatus,
  IssueSeverity,
  Risk,
  Severity,
  SshLogin,
  StatusMeta,
  StepState,
  Tone,
  TriageKind,
  Verdict,
} from "./status";

/** How a triage verdict is shown. */
export function verdictMeta(verdict: string): StatusMeta {
  return lookup(VERDICTS, verdict);
}

/** How a signal's severity is shown. */
export function severityMeta(severity: string): StatusMeta {
  return lookup(SEVERITIES, severity);
}

/** How an incident's status is shown. */
export function incidentStatusMeta(status: string): StatusMeta {
  return lookup(INCIDENT_STATUSES, status);
}

/** Every incident group, most urgent first. */
export const INCIDENT_GROUP_ORDER = Object.keys(INCIDENT_GROUPS) as IncidentGroup[];

/** How a group of the incident list is shown. */
export function incidentGroupMeta(group: string): StatusMeta {
  return lookup(INCIDENT_GROUPS, group);
}

/** The group `incident` is in, by the hub's rule: status first, then triage's verdict. */
export function incidentGroupOf(incident: Pick<IncidentSummary, "status" | "verdict">): string {
  if (incident.status === "acked" || incident.status === "muted") {
    return "handled";
  }
  if (incident.status !== "open") {
    return incident.status;
  }
  if (incident.verdict === "MONITOR") {
    return "monitor";
  }
  return incident.verdict === "EXPECTED" ? "expected" : "needs_action";
}

/**
 * The one pill that says where an incident stands: its status, except that an open incident
 * shows what triage concluded, once it has.
 */
export function incidentStateMeta(
  incident: Pick<IncidentSummary, "status" | "verdict">,
): StatusMeta {
  return incident.status === "open" && incident.verdict
    ? verdictMeta(incident.verdict)
    : incidentStatusMeta(incident.status);
}

/** How an action's status is shown. */
export function actionStatusMeta(status: string): StatusMeta {
  return lookup(ACTION_STATUSES, status);
}

/** How a skill's risk is shown. */
export function riskMeta(risk: string): StatusMeta {
  return lookup(RISKS, risk);
}

/** How the outcome of a triage run is shown. */
export function triageMeta(kind: string): StatusMeta {
  return lookup(TRIAGE, kind);
}

/** How a host's connection is shown. */
export function hostConnectionMeta(connection: string): StatusMeta {
  return lookup(HOST_CONNECTIONS, connection);
}

/** Which connection state `host` is in, for `hostConnectionMeta`. */
export function hostConnectionOf(
  host: Pick<Host, "agent_enrolled" | "connected" | "kind" | "last_seen" | "refused">,
): HostConnection {
  if (host.connected) {
    return "connected";
  }
  if (host.refused) {
    return "refused";
  }
  if (host.kind === "local" && !host.agent_enrolled) {
    return "hub_only";
  }
  return host.last_seen || host.agent_enrolled ? "offline" : "never_connected";
}

/** How the state of one step of a guided flow is shown. */
export function stepStateMeta(state: string): StatusMeta {
  return lookup(STEP_STATES, state);
}

/** How the state of the live event stream is shown. */
export function liveMeta(status: string): StatusMeta {
  return lookup(LIVE, status);
}

/** How the severity of a settings issue is shown. */
export function issueSeverityMeta(severity: string): StatusMeta {
  return lookup(ISSUE_SEVERITIES, severity);
}

/** How a host's kind is shown. */
export function hostKindMeta(kind: string): StatusMeta {
  return lookup(HOST_KINDS, kind);
}

/** How a check's state on a host is shown. */
export function checkStateMeta(state: string): StatusMeta {
  return lookup(CHECK_STATES, state);
}

/** How the result of an SSH login attempt is shown. */
export function sshLoginMeta(result: string): StatusMeta {
  return lookup(SSH_LOGINS, result);
}

/** The one badge an incident shows: Resolved once it resolved, else its verdict or severity. */
export function incidentBadgeMeta(
  incident: Pick<IncidentSummary, "status" | "verdict" | "severity">,
): StatusMeta {
  if (incident.status === "resolved") {
    return INCIDENT_STATUSES.resolved;
  }
  return incident.verdict ? verdictMeta(incident.verdict) : severityMeta(incident.severity);
}

/**
 * How a badge value of unknown vocabulary is shown: the chat card sends a verdict, a
 * severity or "resolved" in one field.
 */
export function badgeMeta(value: string): StatusMeta {
  if (value === "resolved") {
    return INCIDENT_STATUSES.resolved;
  }
  return Object.hasOwn(VERDICTS, value) ? verdictMeta(value) : severityMeta(value);
}

/** Every vocabulary with its known values, for tests and the design catalogue. */
export const VOCABULARIES: {
  meta: (value: string) => StatusMeta;
  name: string;
  values: string[];
}[] = [
  { name: "Verdict", meta: verdictMeta, values: Object.keys(VERDICTS) },
  { name: "Severity", meta: severityMeta, values: Object.keys(SEVERITIES) },
  { name: "Incident status", meta: incidentStatusMeta, values: Object.keys(INCIDENT_STATUSES) },
  { name: "Incident group", meta: incidentGroupMeta, values: Object.keys(INCIDENT_GROUPS) },
  { name: "Action status", meta: actionStatusMeta, values: Object.keys(ACTION_STATUSES) },
  { name: "Skill risk", meta: riskMeta, values: Object.keys(RISKS) },
  { name: "Triage", meta: triageMeta, values: Object.keys(TRIAGE) },
  { name: "Host connection", meta: hostConnectionMeta, values: Object.keys(HOST_CONNECTIONS) },
  { name: "Live updates", meta: liveMeta, values: Object.keys(LIVE) },
  { name: "Settings issue", meta: issueSeverityMeta, values: Object.keys(ISSUE_SEVERITIES) },
  { name: "Host kind", meta: hostKindMeta, values: Object.keys(HOST_KINDS) },
  { name: "Check state", meta: checkStateMeta, values: Object.keys(CHECK_STATES) },
  { name: "SSH login", meta: sshLoginMeta, values: Object.keys(SSH_LOGINS) },
  { name: "Step state", meta: stepStateMeta, values: Object.keys(STEP_STATES) },
];
