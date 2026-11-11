import type { LiveStatus } from "../api/events";
import type { Host, IncidentSummary } from "../api/types";
import type { IconName } from "./icons";

/**
 * The single place where a status becomes a label, a tone, a hint and an icon.
 * Components never decide a status's wording or colour themselves: they ask here.
 */

/** The closed set of status colours; each has `--tone-<tone>-*` tokens. */
export type Tone = "danger" | "warning" | "success" | "pending" | "progress" | "info" | "neutral";

/** Every tone, in the order the design guide lists them. */
export const TONES: Tone[] = [
  "danger",
  "warning",
  "success",
  "pending",
  "progress",
  "info",
  "neutral",
];

/** How a status is shown: its words, its colour, what it means and its icon. */
export interface StatusMeta {
  hint: string;
  icon: IconName;
  label: string;
  tone: Tone;
}

/** What triage concluded about an incident. */
export type Verdict = "ACTION_NEEDED" | "MONITOR" | "EXPECTED" | "FIXED";
/** How bad a signal is. */
export type Severity = "info" | "warning" | "critical";
/** Where an incident is in its life. */
export type IncidentStatus = "pending" | "open" | "acked" | "muted" | "resolved";
/** What an incident asks of the operator; the incident list filters on it. */
export type IncidentGroup =
  "needs_action" | "monitor" | "pending" | "expected" | "handled" | "resolved";
/** Where an action is in its life. */
export type ActionStatus =
  | "proposed"
  | "approved"
  | "denied"
  | "expired"
  | "dispatched"
  | "rejected"
  | "succeeded"
  | "failed"
  | "verified"
  | "verify_failed";
/** What a skill may do. */
export type Risk = "read" | "write" | "destructive";
/** How a triage run went. */
export type TriageKind = "report" | "cached" | "skipped" | "failed";
/** Whether a host's agent is reachable. */
export type HostConnection = "connected" | "offline" | "never_connected" | "hub_only" | "refused";
/** How serious a problem found in submitted settings is. */
export type IssueSeverity = "error" | "warning";
/** What kind of machine a host is. */
export type HostKind = "local" | "agent" | "container_agent" | "external";

type Table<K extends string> = Record<K, StatusMeta>;

/** The meta of `value` in `table`, or a neutral one that shows the raw value: the hub may be newer than the UI. */
function lookup<K extends string>(table: Table<K>, value: string): StatusMeta {
  if (Object.hasOwn(table, value)) {
    return table[value as K];
  }
  return {
    label: value,
    tone: "neutral",
    hint: `"${value}" is not known to this version of the UI.`,
    icon: "dot",
  };
}

const VERDICTS: Table<Verdict> = {
  ACTION_NEEDED: {
    label: "Action needed",
    tone: "danger",
    hint: "Something is broken or will break soon; act now.",
    icon: "alert",
  },
  MONITOR: {
    label: "Monitor",
    tone: "warning",
    hint: "Off but not urgent; keep an eye on it.",
    icon: "eye",
  },
  EXPECTED: {
    label: "Expected",
    tone: "neutral",
    hint: "Explained by something intended, like a planned update or a nightly job.",
    icon: "check",
  },
  FIXED: { label: "Fixed", tone: "success", hint: "Already recovered.", icon: "check-circle" },
};

const SEVERITIES: Table<Severity> = {
  critical: {
    label: "Critical",
    tone: "danger",
    hint: "The check reports a failure that needs attention now.",
    icon: "alert-circle",
  },
  warning: {
    label: "Warning",
    tone: "warning",
    hint: "The check reports something off that is not yet a failure.",
    icon: "alert",
  },
  info: {
    label: "Info",
    tone: "info",
    hint: "The check reports something worth knowing; nothing is wrong.",
    icon: "info",
  },
};

const INCIDENT_STATUSES: Table<IncidentStatus> = {
  pending: {
    label: "Pending",
    tone: "pending",
    hint: "Just opened; waiting to see whether it clears by itself before anyone is notified.",
    icon: "clock",
  },
  open: {
    label: "Open",
    tone: "danger",
    hint: "The problem is still there and nobody has taken it.",
    icon: "alert-circle",
  },
  acked: {
    label: "Acknowledged",
    tone: "neutral",
    hint: "Someone has seen it; reminders are stopped until it changes.",
    icon: "check",
  },
  muted: {
    label: "Muted",
    tone: "neutral",
    hint: "Notifications are silenced for a while; the incident stays open.",
    icon: "mute",
  },
  resolved: {
    label: "Resolved",
    tone: "success",
    hint: "The problem cleared.",
    icon: "check-circle",
  },
};

const INCIDENT_GROUPS: Table<IncidentGroup> = {
  needs_action: {
    label: "Action needed",
    tone: "danger",
    hint: "Open, and nobody has taken it. Triage did not call it expected or only worth watching.",
    icon: "alert",
  },
  monitor: {
    label: "Monitor",
    tone: "warning",
    hint: "Open, and triage said to keep an eye on it.",
    icon: "eye",
  },
  pending: {
    label: "Pending",
    tone: "pending",
    hint: "Not confirmed yet. Domovoi alerts if the check keeps failing.",
    icon: "clock",
  },
  expected: {
    label: "Expected",
    tone: "neutral",
    hint: "Open, but intended: triage or a known condition explains it.",
    icon: "check",
  },
  handled: {
    label: "Acknowledged or muted",
    tone: "neutral",
    hint: "Someone has seen it, or silenced it for a while. It still resolves by itself.",
    icon: "mute",
  },
  resolved: {
    label: "Resolved",
    tone: "success",
    hint: "The problem cleared.",
    icon: "check-circle",
  },
};

const ACTION_STATUSES: Table<ActionStatus> = {
  proposed: {
    label: "Waiting for approval",
    tone: "pending",
    hint: "Nothing runs until a person approves it.",
    icon: "clock",
  },
  approved: {
    label: "Approved",
    tone: "progress",
    hint: "Approved; about to be sent to the host's agent.",
    icon: "loader",
  },
  dispatched: {
    label: "Running",
    tone: "progress",
    hint: "Sent to the host's agent; waiting for the result.",
    icon: "loader",
  },
  succeeded: {
    label: "Succeeded",
    tone: "success",
    hint: "The skill ran and exited cleanly.",
    icon: "check",
  },
  verified: {
    label: "Verified",
    tone: "success",
    hint: "The skill ran and a follow-up check confirmed the fix.",
    icon: "check-circle",
  },
  failed: {
    label: "Failed",
    tone: "danger",
    hint: "The skill ran and reported an error; see its output.",
    icon: "x-circle",
  },
  verify_failed: {
    label: "Not fixed",
    tone: "danger",
    hint: "The skill ran, but the follow-up check still sees the problem.",
    icon: "x-circle",
  },
  rejected: {
    label: "Rejected by agent",
    tone: "danger",
    hint: "The agent refused to run it, for example because the skill is disabled or the arguments are invalid.",
    icon: "ban",
  },
  denied: {
    label: "Denied",
    tone: "neutral",
    hint: "A person declined it; nothing ran.",
    icon: "x",
  },
  expired: {
    label: "Expired",
    tone: "neutral",
    hint: "Nobody approved it in time; nothing ran.",
    icon: "clock",
  },
};

const RISKS: Table<Risk> = {
  read: {
    label: "Read",
    tone: "info",
    hint: "Only looks. Triage may run it on its own.",
    icon: "eye",
  },
  write: {
    label: "Write",
    tone: "warning",
    hint: "Changes something. Runs only after you approve it.",
    icon: "edit",
  },
  destructive: {
    label: "Destructive",
    tone: "danger",
    hint: "Can lose data. Needs the skill name typed to approve; never from chat.",
    icon: "alert",
  },
};

const TRIAGE: Table<TriageKind> = {
  report: {
    label: "Triaged",
    tone: "success",
    hint: "Triage ran and wrote a report.",
    icon: "check",
  },
  cached: {
    label: "Triaged (reused)",
    tone: "success",
    hint: "Triage reused an earlier report for the same evidence.",
    icon: "check",
  },
  skipped: {
    label: "Triage skipped",
    tone: "warning",
    hint: "Triage did not run; the reason is shown with the incident.",
    icon: "pause",
  },
  failed: {
    label: "Triage failed",
    tone: "danger",
    hint: "Triage ran into an error; you can run it again.",
    icon: "x-circle",
  },
};

const HOST_CONNECTIONS: Table<HostConnection> = {
  connected: {
    label: "Connected",
    tone: "success",
    hint: "The agent is connected and reporting.",
    icon: "check-circle",
  },
  offline: {
    label: "Offline",
    tone: "danger",
    hint: "The agent is not connected; checks on this host are not running.",
    icon: "offline",
  },
  never_connected: {
    label: "Never connected",
    tone: "neutral",
    hint: "The agent was enrolled but has not connected yet.",
    icon: "clock",
  },
  hub_only: {
    label: "Hub only, no agent",
    tone: "neutral",
    hint: "The hub records its own problems here. No agent watches this machine.",
    icon: "server",
  },
  refused: {
    label: "Refused",
    tone: "danger",
    hint: "The handshake was refused; update the older side.",
    icon: "ban",
  },
};

const LIVE: Table<LiveStatus> = {
  live: { label: "Live", tone: "success", hint: "Changes appear as they happen.", icon: "dot" },
  connecting: {
    label: "Connecting",
    tone: "progress",
    hint: "Connecting to the hub for live updates.",
    icon: "loader",
  },
  offline: {
    label: "Offline",
    tone: "danger",
    hint: "Live updates are paused; the page may be out of date until the hub answers.",
    icon: "offline",
  },
};

const ISSUE_SEVERITIES: Table<IssueSeverity> = {
  error: {
    label: "Error",
    tone: "danger",
    hint: "Must be fixed before the settings can be saved.",
    icon: "x-circle",
  },
  warning: {
    label: "Warning",
    tone: "warning",
    hint: "The settings can be saved, but this probably is not what you want.",
    icon: "alert",
  },
};

const HOST_KINDS: Table<HostKind> = {
  local: { label: "Hub", tone: "neutral", hint: "The machine the hub runs on.", icon: "server" },
  agent: {
    label: "Agent",
    tone: "neutral",
    hint: "A machine watched by an agent installed on it.",
    icon: "server",
  },
  container_agent: {
    label: "Container agent",
    tone: "neutral",
    hint: "An agent that runs in a container and watches its host through mounts.",
    icon: "server",
  },
  external: {
    label: "External",
    tone: "neutral",
    hint: "Something that reports to the hub through a webhook; no agent runs there.",
    icon: "external",
  },
};

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
];
