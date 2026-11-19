import type { Action, Execution, OperationDetail, OperationSummary } from "../api/types";
import { action } from "./fixtures";
import failedVerification from "./fixtures/operation-trace/failed-verification-new-cycle.json";
import outcomeUnknown from "./fixtures/operation-trace/outcome-unknown-paused.json";
import questionBudget from "./fixtures/operation-trace/question-and-budget-pause.json";
import verifiedSuccess from "./fixtures/operation-trace/verified-success.json";
import waitingApproval from "./fixtures/operation-trace/waiting-approval.json";

/** The five golden trace fixtures, by file name. */
export const TRACE_FIXTURES = {
  "waiting-approval": waitingApproval,
  "verified-success": verifiedSuccess,
  "failed-verification-new-cycle": failedVerification,
  "outcome-unknown-paused": outcomeUnknown,
  "question-and-budget-pause": questionBudget,
} as const;

/** One of the fixture names. */
export type TraceFixtureName = keyof typeof TRACE_FIXTURES;

/** A hash the fixtures use for every plan. */
export const PLAN_HASH = "ab".repeat(32);

/** A step's execution, overridable. */
export function execution(overrides: Partial<Execution> = {}): Execution {
  return {
    step: "step-exec-1",
    approval_step: "step-approval-1",
    contract: "skill.restart_container",
    contract_version: 1,
    host_id: "host-nas",
    check: "container.down",
    target: "plex",
    args: { name: "plex" },
    state: "awaiting_approval",
    plan_hash: PLAN_HASH,
    plan_binding: "v2",
    risk: "write",
    confirm_required: false,
    expires_at: "2099-01-01T00:00:00Z",
    approved_by: null,
    approved_at: null,
    reason: null,
    ...overrides,
  };
}

/** An action the hub marked legacy, overridable. */
export function legacyAction(overrides: Partial<Action> = {}): Action {
  return action({ legacy: true, ...overrides });
}

const STATE_OF_OUTCOME: Record<string, string> = {
  pending: "proposed",
  running: "dispatched",
};

/**
 * An operation detail built from a trace fixture: the summary from the trace, one execution
 * per Execution step (a pending one of a `waiting_approval` operation awaits approval).
 */
export function operationDetail(
  name: TraceFixtureName,
  overrides: Partial<OperationDetail> = {},
): OperationDetail {
  const trace = TRACE_FIXTURES[name] as unknown as OperationDetail["trace"];
  const op = trace.operation;
  const operation: OperationSummary = {
    id: op.id,
    kind: op.kind,
    goal: op.goal,
    status: op.status,
    pause_reason: op.pause_reason ?? null,
    waiting_for: op.waiting_for,
    cycle: op.cycle,
    incidents: op.incidents,
    hosts: op.pinned.map((subject) => subject.host),
    created_at: op.created_at,
    started_at: op.started_at ?? null,
    ended_at: op.ended_at ?? null,
    updated_at: op.ended_at ?? op.started_at ?? op.created_at,
  } as OperationSummary;
  const executions = trace.steps
    .filter((step) => step.kind === "execution")
    .map((step) =>
      execution({
        step: step.id,
        state:
          step.outcome === "pending" && op.status === "waiting_approval"
            ? "awaiting_approval"
            : (STATE_OF_OUTCOME[step.outcome] ?? step.outcome),
      }),
    );
  return { as_of_seq: trace.as_of_seq, operation, trace, executions, ...overrides };
}
