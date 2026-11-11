import type { Table, Verdict, Severity, IncidentStatus, IncidentGroup } from "./status";

/** The vocabularies of what needs the operator: verdicts, severities, incident states and the incident list's groups. */

export const VERDICTS: Table<Verdict> = {
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

export const SEVERITIES: Table<Severity> = {
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

export const INCIDENT_STATUSES: Table<IncidentStatus> = {
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

export const INCIDENT_GROUPS: Table<IncidentGroup> = {
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
