import { DECISION_ERRORS, HARD_LIMITS } from "./policyLimits";
import {
  EFFECT_CLASSES,
  FACT_STATUSES,
  GAP_INPUTS,
  GAP_REASONS,
  SHADOW_REASONS,
} from "./policyRecord";
import {
  COMPARISONS,
  DECISION_REASONS,
  FINDING_KINDS,
  LOCK_CHANGES,
  POLICY_OUTCOMES,
  RULE_MARKS,
  SHADOW_STATUSES,
} from "./policyVocab";
import { lookup, type StatusMeta } from "./status";

/**
 * The lookups for the policy vocabularies, re-exported by `tone.ts`. Import from there.
 */

/** How a decision's outcome is shown. */
export function policyOutcomeMeta(outcome: string): StatusMeta {
  return lookup(POLICY_OUTCOMES, outcome);
}

/** How a mark on a rule or a decision is shown. */
export function ruleMarkMeta(mark: string): StatusMeta {
  return lookup(RULE_MARKS, mark);
}

/** How the reason a decision came out as it did is shown. */
export function decisionReasonMeta(reason: string): StatusMeta {
  return lookup(DECISION_REASONS, reason);
}

/** How a hard limit that applied on top of the rules is shown. */
export function hardLimitMeta(limit: string): StatusMeta {
  return lookup(HARD_LIMITS, limit);
}

/** How a problem found between rules is shown. */
export function findingKindMeta(kind: string): StatusMeta {
  return lookup(FINDING_KINDS, kind);
}

/** How a change to a locked rule is shown. */
export function lockChangeMeta(change: string): StatusMeta {
  return lookup(LOCK_CHANGES, change);
}

/** How the status of a recorded shadow decision is shown. */
export function shadowStatusMeta(status: string): StatusMeta {
  return lookup(SHADOW_STATUSES, status);
}

/** How a replayed decision compares with the recorded one. */
export function comparisonMeta(comparison: string): StatusMeta {
  return lookup(COMPARISONS, comparison);
}

/** How an input missing from a record is shown. */
export function gapInputMeta(input: string): StatusMeta {
  return lookup(GAP_INPUTS, input);
}

/** How the reason an input is missing is shown. */
export function gapReasonMeta(reason: string): StatusMeta {
  return lookup(GAP_REASONS, reason);
}

/** How the reason a shadow decision was not evaluated is shown. */
export function shadowReasonMeta(reason: string): StatusMeta {
  return lookup(SHADOW_REASONS, reason);
}

/** How the error that ended an evaluation is shown. */
export function decisionErrorMeta(code: string): StatusMeta {
  return lookup(DECISION_ERRORS, code);
}

/** How the status of a safety fact is shown. */
export function factStatusMeta(status: string): StatusMeta {
  return lookup(FACT_STATUSES, status);
}

/** How the reach of an action is shown. */
export function effectClassMeta(effect: string): StatusMeta {
  return lookup(EFFECT_CLASSES, effect);
}

/** Every policy vocabulary with its known values, spread into `VOCABULARIES`. */
export const POLICY_VOCABULARIES: {
  meta: (value: string) => StatusMeta;
  name: string;
  values: string[];
}[] = [
  { name: "Policy outcome", meta: policyOutcomeMeta, values: Object.keys(POLICY_OUTCOMES) },
  { name: "Rule mark", meta: ruleMarkMeta, values: Object.keys(RULE_MARKS) },
  { name: "Decision reason", meta: decisionReasonMeta, values: Object.keys(DECISION_REASONS) },
  { name: "Hard limit", meta: hardLimitMeta, values: Object.keys(HARD_LIMITS) },
  { name: "Policy finding", meta: findingKindMeta, values: Object.keys(FINDING_KINDS) },
  { name: "Lock change", meta: lockChangeMeta, values: Object.keys(LOCK_CHANGES) },
  { name: "Shadow status", meta: shadowStatusMeta, values: Object.keys(SHADOW_STATUSES) },
  { name: "Replay comparison", meta: comparisonMeta, values: Object.keys(COMPARISONS) },
  { name: "Replay gap input", meta: gapInputMeta, values: Object.keys(GAP_INPUTS) },
  { name: "Replay gap reason", meta: gapReasonMeta, values: Object.keys(GAP_REASONS) },
  { name: "Shadow reason", meta: shadowReasonMeta, values: Object.keys(SHADOW_REASONS) },
  { name: "Decision error", meta: decisionErrorMeta, values: Object.keys(DECISION_ERRORS) },
  { name: "Fact status", meta: factStatusMeta, values: Object.keys(FACT_STATUSES) },
  { name: "Effect class", meta: effectClassMeta, values: Object.keys(EFFECT_CLASSES) },
];
