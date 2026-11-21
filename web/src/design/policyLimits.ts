import type { Table } from "./status";

/**
 * The words for what stops or limits a decision outside the rules: the hard limits that
 * apply on top of them, and the errors that end an evaluation. Read only through the
 * lookups in `tone.ts`.
 */

/** A hard limit that applied on top of the rules. */
export type HardLimit =
  | "destructive_never_allowed"
  | "target_not_pinned"
  | "safety_fact_unknown"
  | "ceiling_does_not_cover"
  | "containment_unverified"
  | "management_path_uncovered"
  | "containment_target_excluded"
  | "helper_safety_incomplete"
  | "autonomy_inactive"
  | "agent_runs_as_root";

/** Why the rules could not be evaluated at all. */
export type DecisionError =
  | "locked_not_deny"
  | "unavailable_rule_outcome"
  | "duplicate_rule_id"
  | "empty_target_set"
  | "degenerate_time_window"
  | "invalid_time_of_day"
  | "negative_max_ttl"
  | "bad_arg_key"
  | "unknown_contract"
  | "category_not_in_index"
  | "contract_not_indexed"
  | "containment_on_non_contain"
  | "ceiling_hash_missing"
  | "too_many_facts";

export const HARD_LIMITS: Table<HardLimit> = {
  destructive_never_allowed: {
    label: "Destructive actions are never automatic",
    tone: "warning",
    icon: "alert",
    hint: "A destructive action is always offered for approval, whatever the rules say.",
  },
  target_not_pinned: {
    label: "Target is outside the pinned subjects",
    tone: "warning",
    icon: "ban",
    hint: "The action was not allowed because its target is not one the host pinned.",
  },
  safety_fact_unknown: {
    label: "A safety check could not be confirmed",
    tone: "warning",
    icon: "help",
    hint: "A fact the action depends on was unknown, so it was not allowed to run on its own.",
  },
  ceiling_does_not_cover: {
    label: "The host's own ceiling does not cover it",
    tone: "warning",
    icon: "shield",
    hint: "The host limits what may run on it, and this action is beyond that limit.",
  },
  containment_unverified: {
    label: "Protected paths could not be verified",
    tone: "warning",
    icon: "shield",
    hint: "Whether the action stays clear of protected paths could not be checked.",
  },
  management_path_uncovered: {
    label: "Some management paths are not protected",
    tone: "warning",
    icon: "shield",
    hint: "The paths that manage the host are not all covered, so the action was held back.",
  },
  containment_target_excluded: {
    label: "The target is excluded from containment",
    tone: "danger",
    icon: "ban",
    hint: "The target is excluded from containment, so the action is not allowed to run on its own.",
  },
  helper_safety_incomplete: {
    label: "The helper cannot compute its safety set",
    tone: "warning",
    icon: "alert",
    hint: "The host's helper did not report what it protects, so the action was held back.",
  },
  autonomy_inactive: {
    label: "Autonomy is paused or a rule is suspended",
    tone: "warning",
    icon: "pause",
    hint: "Automatic actions are paused, so nothing runs without approval.",
  },
  agent_runs_as_root: {
    label: "The agent runs as root",
    tone: "warning",
    icon: "alert",
    hint: "The host's agent has full privileges, so automatic actions are held back.",
  },
};

export const DECISION_ERRORS: Table<DecisionError> = {
  locked_not_deny: {
    label: "A locked rule is not a Deny",
    tone: "warning",
    icon: "alert",
    hint: "Only a Deny can be locked, so a locked rule with another outcome is invalid.",
  },
  unavailable_rule_outcome: {
    label: "A rule has an outcome it cannot have",
    tone: "warning",
    icon: "alert",
    hint: "A rule may only Allow, Ask for approval or Deny.",
  },
  duplicate_rule_id: {
    label: "Two rules share an identifier",
    tone: "warning",
    icon: "alert",
    hint: "Every rule needs its own identifier, so one of them has to change.",
  },
  empty_target_set: {
    label: "A rule names no targets",
    tone: "warning",
    icon: "alert",
    hint: "A rule that lists exact targets must list at least one.",
  },
  degenerate_time_window: {
    label: "A time window starts and ends together",
    tone: "warning",
    icon: "alert",
    hint: "A window with the same start and end covers nothing, so it needs different times.",
  },
  invalid_time_of_day: {
    label: "A time of day is not valid",
    tone: "warning",
    icon: "alert",
    hint: "Times are hours from 00 to 23 and minutes from 00 to 59.",
  },
  negative_max_ttl: {
    label: "A maximum lifetime is negative",
    tone: "warning",
    icon: "alert",
    hint: "A maximum lifetime must be zero or more seconds.",
  },
  bad_arg_key: {
    label: "An argument name is not valid",
    tone: "warning",
    icon: "alert",
    hint: "A condition on an argument names one the action does not take.",
  },
  unknown_contract: {
    label: "A rule names an unknown capability",
    tone: "warning",
    icon: "alert",
    hint: "No capability has the name a rule uses, so the rule cannot apply.",
  },
  category_not_in_index: {
    label: "A rule names an unknown category",
    tone: "warning",
    icon: "alert",
    hint: "No capability belongs to the category a rule uses.",
  },
  contract_not_indexed: {
    label: "A capability is missing from the index",
    tone: "warning",
    icon: "alert",
    hint: "The hub does not list this capability, so the rules cannot be evaluated for it.",
  },
  containment_on_non_contain: {
    label: "Containment is set on a non-containing action",
    tone: "warning",
    icon: "alert",
    hint: "Only actions that contain a problem can carry a containment setting.",
  },
  ceiling_hash_missing: {
    label: "The host's ceiling is missing",
    tone: "warning",
    icon: "alert",
    hint: "The host did not report its ceiling, so the rules cannot be checked against it.",
  },
  too_many_facts: {
    label: "The decision needs too many facts",
    tone: "warning",
    icon: "alert",
    hint: "More safety facts were needed than one decision can hold.",
  },
};
