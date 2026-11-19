import type { Table } from "./status";

export { EDGE_KINDS, STEP_KINDS } from "./operationKindVocab";
export type { EdgeKind, StepKind } from "./operationKindVocab";

/**
 * The words for operations: their status, a step's outcome, an execution's state, a step's
 * kind and an edge's kind. Read only through the lookups in `tone.ts`.
 */

/** Where an operation is in its life. */
export type OperationStatus =
  | "queued"
  | "running"
  | "waiting_approval"
  | "waiting_user"
  | "waiting_agent"
  | "waiting_timer"
  | "paused"
  | "succeeded"
  | "failed"
  | "escalated"
  | "cancelled";

/** How one step ended, or that it has not. */
export type StepOutcome =
  | "pending"
  | "running"
  | "succeeded"
  | "failed"
  | "refused"
  | "not_run"
  | "unavailable"
  | "outcome_unknown"
  | "skipped"
  | "cancelled";

/** Where an execution is, from proposal to result. */
export type ExecutionState =
  | "proposed"
  | "awaiting_approval"
  | "authorized"
  | "dispatched"
  | "refused"
  | "not_run"
  | "unavailable"
  | "succeeded"
  | "failed"
  | "outcome_unknown"
  | "skipped"
  | "cancelled";

export const OPERATION_STATUSES: Table<OperationStatus> = {
  queued: {
    label: "Queued",
    tone: "neutral",
    icon: "clock",
    hint: "Waiting for a free slot; it starts on its own.",
  },
  running: {
    label: "Running",
    tone: "progress",
    icon: "loader",
    hint: "Domovoi is working on it now.",
  },
  waiting_approval: {
    label: "Waiting for approval",
    tone: "pending",
    icon: "clock",
    hint: "Nothing runs until a person approves the proposed action.",
  },
  waiting_user: {
    label: "Waiting for your answer",
    tone: "pending",
    icon: "help",
    hint: "Domovoi asked a question and continues once you answer.",
  },
  waiting_agent: {
    label: "Waiting for the host",
    tone: "info",
    icon: "offline",
    hint: "A host's agent is not connected; it continues when the agent returns.",
  },
  waiting_timer: {
    label: "Waiting for a check",
    tone: "info",
    icon: "clock",
    hint: "A follow-up check is scheduled; it continues when it is due.",
  },
  paused: {
    label: "Paused",
    tone: "warning",
    icon: "pause",
    hint: "It stopped and does nothing until a person resumes it.",
  },
  succeeded: {
    label: "Succeeded",
    tone: "success",
    icon: "check-circle",
    hint: "It finished and reached its goal.",
  },
  failed: {
    label: "Failed",
    tone: "danger",
    icon: "x-circle",
    hint: "It finished without reaching its goal.",
  },
  escalated: {
    label: "Escalated",
    tone: "danger",
    icon: "alert",
    hint: "Domovoi stopped and handed it to a person.",
  },
  cancelled: {
    label: "Cancelled",
    tone: "neutral",
    icon: "ban",
    hint: "A person cancelled it.",
  },
};

export const STEP_OUTCOMES: Table<StepOutcome> = {
  pending: { label: "Not started", tone: "neutral", icon: "clock", hint: "Waiting its turn." },
  running: { label: "Running", tone: "progress", icon: "loader", hint: "In progress now." },
  succeeded: {
    label: "Succeeded",
    tone: "success",
    icon: "check",
    hint: "It finished as intended.",
  },
  failed: {
    label: "Failed",
    tone: "danger",
    icon: "x-circle",
    hint: "It ran and reported a failure.",
  },
  refused: {
    label: "Refused",
    tone: "danger",
    icon: "ban",
    hint: "Something refused to run it, such as a check, a person or the agent.",
  },
  not_run: {
    label: "Never ran",
    tone: "neutral",
    icon: "ban",
    hint: "Nothing was started: the approval expired, or the host has no record of it.",
  },
  unavailable: {
    label: "Unavailable",
    tone: "warning",
    icon: "offline",
    hint: "What it needs was not available, such as the host or a capability.",
  },
  outcome_unknown: {
    label: "Result unknown",
    tone: "warning",
    icon: "alert",
    hint: "It may have run on the host; check the host before running it again.",
  },
  skipped: {
    label: "Skipped",
    tone: "neutral",
    icon: "skip",
    hint: "It was not needed and was left out.",
  },
  cancelled: {
    label: "Cancelled",
    tone: "neutral",
    icon: "x",
    hint: "It was cancelled before it finished.",
  },
};

export const EXECUTION_STATES: Table<ExecutionState> = {
  proposed: {
    label: "Proposed",
    tone: "neutral",
    icon: "clock",
    hint: "Proposed; its approval is not open yet.",
  },
  awaiting_approval: {
    label: "Waiting for approval",
    tone: "pending",
    icon: "clock",
    hint: "Nothing runs until a person approves it.",
  },
  authorized: {
    label: "Approved",
    tone: "progress",
    icon: "loader",
    hint: "Approved; about to be sent to the host's agent.",
  },
  dispatched: {
    label: "Running",
    tone: "progress",
    icon: "loader",
    hint: "Sent to the host's agent; waiting for the result.",
  },
  refused: {
    label: "Refused",
    tone: "danger",
    icon: "ban",
    hint: "A person denied it, or the plan no longer fit. Nothing ran.",
  },
  not_run: {
    label: "Never ran",
    tone: "neutral",
    icon: "ban",
    hint: "Nobody approved it in time, or the agent has no record of it. Nothing ran.",
  },
  unavailable: {
    label: "Unavailable",
    tone: "warning",
    icon: "offline",
    hint: "The host or the capability was not available. Nothing ran.",
  },
  succeeded: STEP_OUTCOMES.succeeded,
  failed: STEP_OUTCOMES.failed,
  outcome_unknown: STEP_OUTCOMES.outcome_unknown,
  skipped: STEP_OUTCOMES.skipped,
  cancelled: STEP_OUTCOMES.cancelled,
};
