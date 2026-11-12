import type { IconName } from "./icons";

/**
 * The shapes every status vocabulary shares: the closed set of tones, what a status looks
 * like, the value types of each vocabulary, and the lookup that tolerates a value this
 * version of the UI does not know. The tables themselves are in the `*Vocab.ts` files and
 * are read only through `tone.ts`.
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
/** How a container runs, as its host reports it. */
export type ContainerState =
  "healthy" | "running" | "unhealthy" | "restarting" | "exited" | "other";
/** How serious a problem found in submitted settings is. */
export type IssueSeverity = "error" | "warning";
/** What kind of machine a host is. */
export type HostKind = "local" | "agent" | "container_agent" | "external";
/** Where a check stands on a host. */
export type CheckState = "ok" | "failing" | "disabled";
/** How one SSH login attempt ended; `many_failures` is a failed one from an address that keeps failing. */
export type SshLogin = "accepted" | "failed" | "many_failures";
/** Where one step of a guided flow stands. */
export type StepState = "done" | "current" | "skipped" | "upcoming";

/** One vocabulary: every known value and how it is shown. */
export type Table<K extends string> = Record<K, StatusMeta>;

/** The meta of `value` in `table`, or a neutral one that shows the raw value: the hub may be newer than the UI. */
export function lookup<K extends string>(table: Table<K>, value: string): StatusMeta {
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
