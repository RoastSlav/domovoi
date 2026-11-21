import type { Table } from "./status";

/**
 * The words for how a decision was recorded and replayed: the inputs that were missing,
 * why they were missing and why a shadow decision was not evaluated. Read only through
 * the lookups in `tone.ts`.
 */

/** How well a safety fact is known. */
export type FactStatus = "known" | "unknown" | "stale" | "disputed";

/** How far an action reaches. */
export type EffectClass = "read" | "mutate" | "contain" | "destructive";

/** An input a replay could not take from the record. */
export type GapInput =
  | "contract"
  | "target"
  | "ceiling"
  | "facts"
  | "runs_as_root"
  | "containment"
  | "autonomy"
  | "pinned_targets";

/** Why an input is missing from the record. */
export type GapReason = "not_produced_yet" | "host_not_connected" | "unresolvable";

/** Why a shadow decision was not evaluated. */
export type ShadowReason =
  | "contract_unknown"
  | "target_unresolved"
  | "target_kind_unsupported"
  | "package_contract"
  | "too_large"
  | "late";

export const GAP_INPUTS: Table<GapInput> = {
  contract: {
    label: "Capability",
    tone: "neutral",
    icon: "info",
    hint: "Which capability the action used was not recorded.",
  },
  target: {
    label: "Target",
    tone: "neutral",
    icon: "info",
    hint: "What the action was aimed at was not recorded.",
  },
  ceiling: {
    label: "Host ceiling",
    tone: "neutral",
    icon: "info",
    hint: "What the host allowed at the time was not recorded.",
  },
  facts: {
    label: "Safety facts",
    tone: "neutral",
    icon: "info",
    hint: "The safety facts known at the time were not recorded.",
  },
  runs_as_root: {
    label: "Agent privilege",
    tone: "neutral",
    icon: "info",
    hint: "Whether the agent ran as root was not recorded.",
  },
  containment: {
    label: "Containment cover",
    tone: "neutral",
    icon: "info",
    hint: "Which paths were protected at the time was not recorded.",
  },
  autonomy: {
    label: "Autonomy state",
    tone: "neutral",
    icon: "info",
    hint: "Whether automatic actions were paused was not recorded.",
  },
  pinned_targets: {
    label: "Pinned targets",
    tone: "neutral",
    icon: "info",
    hint: "Which targets the host pinned was not recorded.",
  },
};

export const GAP_REASONS: Table<GapReason> = {
  not_produced_yet: {
    label: "Not available yet",
    tone: "warning",
    icon: "alert",
    hint: "This version of the hub did not record this input when the decision was made.",
  },
  host_not_connected: {
    label: "Host not connected",
    tone: "warning",
    icon: "alert",
    hint: "The host's agent was not connected, so it could not report this input.",
  },
  unresolvable: {
    label: "Could not be established",
    tone: "warning",
    icon: "alert",
    hint: "The input could not be worked out when the decision was made.",
  },
};

export const SHADOW_REASONS: Table<ShadowReason> = {
  contract_unknown: {
    label: "Capability not known",
    tone: "neutral",
    icon: "info",
    hint: "The hub did not know the capability the action used, so it could not be evaluated.",
  },
  target_unresolved: {
    label: "Target not resolved",
    tone: "neutral",
    icon: "info",
    hint: "The action's target could not be worked out, so it could not be evaluated.",
  },
  target_kind_unsupported: {
    label: "Target kind not supported",
    tone: "neutral",
    icon: "info",
    hint: "Rules cannot yet describe this kind of target, so it was not evaluated.",
  },
  package_contract: {
    label: "Package action, not evaluated",
    tone: "neutral",
    icon: "info",
    hint: "Package actions follow their own approval path and are not evaluated here.",
  },
  too_large: {
    label: "Record too large",
    tone: "neutral",
    icon: "info",
    hint: "The decision record was too large to keep, so it was not evaluated.",
  },
  late: {
    label: "Evaluated late",
    tone: "neutral",
    icon: "info",
    hint: "The evaluation ran after the action, so the state it saw may differ.",
  },
};

export const FACT_STATUSES: Table<FactStatus> = {
  known: {
    label: "Known",
    tone: "success",
    icon: "check",
    hint: "The fact was confirmed when the decision was made.",
  },
  unknown: {
    label: "Unknown",
    tone: "warning",
    icon: "help",
    hint: "The fact could not be confirmed when the decision was made.",
  },
  stale: {
    label: "Stale",
    tone: "warning",
    icon: "clock",
    hint: "The fact was confirmed once but is too old to rely on.",
  },
  disputed: {
    label: "Disputed",
    tone: "warning",
    icon: "alert",
    hint: "Two sources disagree about the fact.",
  },
};

export const EFFECT_CLASSES: Table<EffectClass> = {
  read: {
    label: "Read",
    tone: "info",
    icon: "eye",
    hint: "It only looks at the host and changes nothing.",
  },
  mutate: {
    label: "Mutate",
    tone: "neutral",
    icon: "edit",
    hint: "It changes something on the host and can be undone.",
  },
  contain: {
    label: "Contain",
    tone: "warning",
    icon: "shield",
    hint: "It limits the damage of a problem, such as stopping or isolating something.",
  },
  destructive: {
    label: "Destructive",
    tone: "danger",
    icon: "trash",
    hint: "It removes something that may not come back.",
  },
};
