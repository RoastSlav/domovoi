import type { Table, ActionStatus, Risk, TriageKind } from "./status";

/** The vocabularies of what runs: actions, skill risks and triage runs. */

export const ACTION_STATUSES: Table<ActionStatus> = {
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

export const RISKS: Table<Risk> = {
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

export const TRIAGE: Table<TriageKind> = {
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
