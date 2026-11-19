/**
 * The operation trace as the hub writes it. Hand-written, not generated: the trace is a
 * frozen, versioned schema (`schema_version`). Adding a value to a union is not breaking, so
 * every union also admits any other string through `(string & {})`; a reader must not
 * assume the list is closed.
 */

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

/** Why an operation is paused. The tag is `type`. */
export type PauseReason =
  | { type: "budget"; exhausted: BudgetKind }
  | { type: "outcome_unknown"; step: string }
  | { type: "global_pause" }
  | { type: "error"; message: string };

/** The limits set for an operation. `null` means no limit. */
export interface BudgetLimits {
  model_turns: number | null;
  evidence_requests: number | null;
  executions: number | null;
  cycles: number | null;
  wall_seconds: number | null;
  tokens: number | null;
  cost_micros: number | null;
}

/** What an operation has spent so far. */
export interface BudgetUsed {
  model_turns: number;
  evidence_requests: number;
  executions: number;
  wall_seconds: number;
  tokens: number;
  cost_micros: number;
}

/** Limits and use together. */
export interface Budgets {
  limits: BudgetLimits;
  used: BudgetUsed;
}

/** One thing an operation may act on: a check on a target on a host. */
export interface PinnedSubject {
  host: string;
  check: string;
  target: string | null;
}

/** The operation part of a trace. */
export interface TraceOperation {
  id: string;
  kind: OperationKind;
  goal: string;
  status: OperationStatus;
  pause_reason: PauseReason | null;
  waiting_for: WaitKind[];
  cycle: number;
  budgets: Budgets;
  pinned: PinnedSubject[];
  parent: string | null;
  incidents: string[];
  created_at: string;
  started_at: string | null;
  ended_at: string | null;
}

/** A link from a step to something else, such as its execution. */
export interface TraceRef {
  kind: "execution" | (string & {});
  id: string;
}

/** One step in a trace. Steps are listed in ordinal order. */
export interface TraceStep {
  id: string;
  parent: string | null;
  kind: StepKind;
  phase: string;
  cycle: number;
  outcome: StepOutcome;
  started_at: string | null;
  ended_at: string | null;
  summary: string | null;
  refs: TraceRef[];
}

/** One edge in a trace. `from` is the later step, `to` the earlier one. */
export interface TraceEdge {
  from: string;
  to: string;
  kind: EdgeKind;
}

/** The frozen read-only view of one operation. */
export interface OperationTrace {
  schema_version: number;
  as_of_seq: number;
  operation: TraceOperation;
  steps: TraceStep[];
  edges: TraceEdge[];
}
