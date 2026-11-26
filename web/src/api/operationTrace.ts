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

/** What an open step is waiting for, in priority order. */
export const WAIT_KINDS = ["approval", "user", "agent", "timer"] as const;

/** The frozen read-only view of one operation. */
export type OperationTrace = components["schemas"]["OperationTraceDto"];
/** One step in a trace. Steps are listed in ordinal order. */
export type TraceStep = components["schemas"]["TraceStepDto"];
/** One edge in a trace. `from` is the later step, `to` the earlier one. */
export type TraceEdge = components["schemas"]["TraceEdgeDto"];
/** Why an operation is paused. The tag is `type`. */
export type PauseReason = components["schemas"]["PauseReasonDto"];
