import type { Table } from "./status";

/**
 * The words for policy: what a decision was, which marks a rule carries, why a rule won,
 * what a publication changes and how a recorded decision compares. Read only through the
 * lookups in `tone.ts`.
 */

/** What a decision came to. `error` is a decision that could not be evaluated at all. */
export type PolicyOutcome = "allow" | "require_approval" | "deny" | "unavailable" | "error";

/** A mark a rule can carry, or a warning a decision can carry. */
export type RuleMark =
  "locked_deny" | "shadow" | "disabled" | "dangerous_exception" | "not_enforced";

/** Why the decision came out as it did. */
export type DecisionReason =
  "default" | "single_winner" | "equal_scope_deny" | "incomparable_fallback" | "locked_deny";

/** A problem the publication check found between rules. */
export type FindingKind =
  | "equal_scope_conflict"
  | "incomparable_conflict"
  | "ineffective_under_lock"
  | "target_outside_every_ceiling";

/** How a draft changes a locked rule. */
export type LockChange = "created" | "edited" | "unlocked" | "removed";

/** How a recorded shadow decision ended. */
export type ShadowStatus = "evaluated" | "unevaluable" | "skipped";

/** How a re-evaluated decision compares with the recorded one. */
export type Comparison = "same" | "changed" | "not_replayable";

export const POLICY_OUTCOMES: Table<PolicyOutcome> = {
  allow: {
    label: "Allow automatically",
    tone: "success",
    icon: "check-circle",
    hint: "It runs without asking you, within the limits shown.",
  },
  require_approval: {
    label: "Ask for approval",
    tone: "pending",
    icon: "clock",
    hint: "You are offered the action and decide.",
  },
  deny: {
    label: "Deny",
    tone: "neutral",
    icon: "ban",
    hint: "It is never offered. To allow it, change the policy.",
  },
  unavailable: {
    label: "Cannot decide",
    tone: "warning",
    icon: "alert",
    hint: "A safety input could not be confirmed. Nothing runs and nothing is offered.",
  },
  error: {
    label: "Could not evaluate",
    tone: "warning",
    icon: "alert",
    hint: "The rules could not be evaluated, so no decision was reached.",
  },
};

export const RULE_MARKS: Table<RuleMark> = {
  locked_deny: {
    label: "Locked Deny",
    tone: "danger",
    icon: "lock",
    hint: "An absolute prohibition. No narrower rule can override it, and changing it needs your password.",
  },
  shadow: {
    label: "Shadow",
    tone: "info",
    icon: "eye",
    hint: "Recorded for comparison and never acted on.",
  },
  disabled: {
    label: "Disabled",
    tone: "neutral",
    icon: "pause",
    hint: "Ignored: it neither matches nor explains.",
  },
  dangerous_exception: {
    label: "Allow overrides a broader Deny",
    tone: "warning",
    icon: "alert",
    hint: "A specific Allow beats a broader Deny. This is deliberate when you wrote it, and dangerous if you did not.",
  },
  not_enforced: {
    label: "Not enforced",
    tone: "info",
    icon: "info",
    hint: "Recorded for comparison. No approval, dispatch or limit reads it.",
  },
};

export const DECISION_REASONS: Table<DecisionReason> = {
  default: {
    label: "Default for this kind of action",
    tone: "neutral",
    icon: "info",
    hint: "No rule matched, so the default for this kind of action applies.",
  },
  single_winner: {
    label: "Most specific rule",
    tone: "neutral",
    icon: "info",
    hint: "One matching rule is more specific than the others, so it wins.",
  },
  equal_scope_deny: {
    label: "Equal scope, Deny wins",
    tone: "neutral",
    icon: "info",
    hint: "Matching rules are equally specific and one denies, so Deny wins.",
  },
  incomparable_fallback: {
    label: "Conflicting rules, safer outcome",
    tone: "neutral",
    icon: "info",
    hint: "Neither rule is more specific, so the safer outcome applies: Deny, then Ask for approval, then Allow.",
  },
  locked_deny: {
    label: "A locked rule applies",
    tone: "neutral",
    icon: "info",
    hint: "A locked Deny matched, and it overrides every other rule.",
  },
};

export const FINDING_KINDS: Table<FindingKind> = {
  equal_scope_conflict: {
    label: "Equal-scope conflict",
    tone: "warning",
    icon: "alert",
    hint: "Two rules are equally specific and disagree, so the safer one wins.",
  },
  incomparable_conflict: {
    label: "Conflicting overlap",
    tone: "warning",
    icon: "alert",
    hint: "Two rules overlap and neither is more specific, so the safer outcome wins.",
  },
  ineffective_under_lock: {
    label: "Overridden by a locked rule",
    tone: "danger",
    icon: "lock",
    hint: "A locked rule always wins here, so this rule can never take effect.",
  },
  target_outside_every_ceiling: {
    label: "Outside every host ceiling",
    tone: "info",
    icon: "info",
    hint: "No host's ceiling covers this target, so the rule cannot allow it anywhere.",
  },
};

export const LOCK_CHANGES: Table<LockChange> = {
  created: {
    label: "Locked rule added",
    tone: "danger",
    icon: "lock",
    hint: "This version adds an absolute prohibition.",
  },
  edited: {
    label: "Locked rule changed",
    tone: "danger",
    icon: "lock",
    hint: "This version changes what a locked rule prohibits.",
  },
  unlocked: {
    label: "Rule unlocked",
    tone: "danger",
    icon: "lock",
    hint: "This version turns a locked rule into one that narrower rules can override.",
  },
  removed: {
    label: "Locked rule removed",
    tone: "danger",
    icon: "lock",
    hint: "This version drops an absolute prohibition.",
  },
};

export const SHADOW_STATUSES: Table<ShadowStatus> = {
  evaluated: {
    label: "Recorded",
    tone: "success",
    icon: "check-circle",
    hint: "The shadow rules were evaluated and the result was recorded.",
  },
  unevaluable: {
    label: "Could not evaluate",
    tone: "warning",
    icon: "alert",
    hint: "The shadow rules could not be evaluated for this action.",
  },
  skipped: {
    label: "Skipped",
    tone: "neutral",
    icon: "skip",
    hint: "This action was not evaluated against the shadow rules.",
  },
};

export const COMPARISONS: Table<Comparison> = {
  same: {
    label: "Same",
    tone: "success",
    icon: "check",
    hint: "Re-evaluating gives the outcome that was recorded.",
  },
  changed: {
    label: "Changed",
    tone: "warning",
    icon: "alert",
    hint: "Re-evaluating gives a different outcome from the one recorded.",
  },
  not_replayable: {
    label: "Cannot replay",
    tone: "neutral",
    icon: "help",
    hint: "The record does not hold enough to evaluate this decision again.",
  },
};
