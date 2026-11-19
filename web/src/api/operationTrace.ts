/**
 * The operation trace as the hub writes it. The shapes are the generated API schema; the
 * enumerations the API sends as plain strings are listed here, because the trace is a frozen,
 * versioned schema (`schema_version`). Adding a value is not breaking, so every union also
 * admits any other string through `(string & {})`; a reader must not assume the list is
 * closed.
 */
import type { components } from "./schema";

/** The step kinds the trace schema names today. */
export const STEP_KINDS = [
  "reasoning_turn",
  "evidence_request",
  "assessment",
  "plan",
  "policy_decision",
  "approval",
  "execution",
  "verification",
  "notification",
  "question",
  "note",
] as const;

/** How far a step got. */
export const STEP_OUTCOMES = [
  "pending",
  "running",
  "succeeded",
  "failed",
  "refused",
  "not_run",
  "unavailable",
  "outcome_unknown",
  "skipped",
  "cancelled",
] as const;

/** How two steps relate. */
export const EDGE_KINDS = ["depends_on", "caused_by", "verifies", "retry_of", "answers"] as const;

/** What an operation is for. */
export const OPERATION_KINDS = ["investigate"] as const;

/** The coarse status of an operation. */
export const OPERATION_STATUSES = [
  "queued",
  "running",
  "waiting_approval",
  "waiting_user",
  "waiting_agent",
  "waiting_timer",
  "paused",
  "succeeded",
  "failed",
  "escalated",
  "cancelled",
] as const;

/** What an open step is waiting for, in priority order. */
export const WAIT_KINDS = ["approval", "user", "agent", "timer"] as const;

/** A budget that can run out. */
export const BUDGET_KINDS = [
  "model_turns",
  "evidence_requests",
  "executions",
  "cycles",
  "wall_time",
  "tokens",
  "cost",
] as const;

/** A step kind. */
export type StepKind = (typeof STEP_KINDS)[number] | (string & {});
/** A step outcome. */
export type StepOutcome = (typeof STEP_OUTCOMES)[number] | (string & {});
/** An edge kind. */
export type EdgeKind = (typeof EDGE_KINDS)[number] | (string & {});
/** An operation kind. */
export type OperationKind = (typeof OPERATION_KINDS)[number] | (string & {});
/** An operation status. */
export type OperationStatus = (typeof OPERATION_STATUSES)[number] | (string & {});
/** A wait kind. */
export type WaitKind = (typeof WAIT_KINDS)[number] | (string & {});
/** A budget kind. */
export type BudgetKind = (typeof BUDGET_KINDS)[number] | (string & {});

/** The frozen read-only view of one operation. */
export type OperationTrace = components["schemas"]["OperationTraceDto"];
/** The operation part of a trace. */
export type TraceOperation = components["schemas"]["TraceOperationDto"];
/** One step in a trace. Steps are listed in ordinal order. */
export type TraceStep = components["schemas"]["TraceStepDto"];
/** One edge in a trace. `from` is the later step, `to` the earlier one. */
export type TraceEdge = components["schemas"]["TraceEdgeDto"];
/** A link from a step to something else, such as its execution. */
export type TraceRef = components["schemas"]["TraceRefDto"];
/** Why an operation is paused. The tag is `type`. */
export type PauseReason = components["schemas"]["PauseReasonDto"];
/** The limits set for an operation. `null` means no limit. */
export type BudgetLimits = components["schemas"]["BudgetLimitsDto"];
/** What an operation has spent so far. */
export type BudgetUsed = components["schemas"]["BudgetUsedDto"];
/** Limits and use together. */
export type Budgets = components["schemas"]["BudgetsDto"];
/** One thing an operation may act on: a check on a target on a host. */
export type PinnedSubject = components["schemas"]["PinnedSubjectDto"];
