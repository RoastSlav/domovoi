import type { OperationTrace } from "../api/operationTrace";
import { floorSecond } from "../features/operations/replay/frames";
import type { HistoryEvent, OperationHistory } from "../features/operations/replay/types";
import failedVerification from "./fixtures/operation-trace/failed-verification-new-cycle.json";
import outcomeUnknown from "./fixtures/operation-trace/outcome-unknown-paused.json";
import questionAndBudget from "./fixtures/operation-trace/question-and-budget-pause.json";
import verifiedSuccess from "./fixtures/operation-trace/verified-success.json";
import waitingApproval from "./fixtures/operation-trace/waiting-approval.json";

/** The five operation-trace fixtures by name. */
export const TRACE_FIXTURES: Record<string, OperationTrace> = {
  "failed-verification-new-cycle": failedVerification as OperationTrace,
  "outcome-unknown-paused": outcomeUnknown as OperationTrace,
  "question-and-budget-pause": questionAndBudget as OperationTrace,
  "verified-success": verifiedSuccess as OperationTrace,
  "waiting-approval": waitingApproval as OperationTrace,
};

/** One history row with the fields a test cares about. */
export function ev(
  seq: number,
  at: string,
  kind: string,
  step: string | null = null,
  state: string | null = null,
): HistoryEvent {
  return { seq, at, kind, step, state };
}

/** Wrap rows in a history; `complete` is true unless a reason is given. */
export function historyOf(
  events: HistoryEvent[],
  incompleteReason: string | null = null,
): OperationHistory {
  return {
    as_of_seq: events[events.length - 1]?.seq ?? 0,
    complete: incompleteReason === null,
    incomplete_reason: incompleteReason,
    events,
  };
}

/**
 * A complete history consistent with a trace: the creation row at the operation's creation, one
 * `step_created` row per step at its `started_at` (else the previous step's time plus a second),
 * a `step_finished` row at each `ended_at`, a `dispatched` row for an execution step that has a
 * `started_at`, and `running` and final status rows. Rows are ordered by time, then by creation.
 */
export function historyFor(trace: OperationTrace): OperationHistory {
  const sec = (value: string | null | undefined): number => floorSecond(value) ?? 0;
  const iso = (ms: number): string => new Date(ms).toISOString().replace(".000Z", "Z");
  const rows: { ms: number; kind: string; step: string | null; state: string | null }[] = [];
  const created = sec(trace.operation.created_at);
  rows.push({ ms: created, kind: "operation.created", step: null, state: null });
  rows.push({ ms: created, kind: "operation.status", step: null, state: "running" });
  let previous = created;
  let last = created;
  for (const step of trace.steps) {
    const at = step.started_at === null ? previous + 1000 : sec(step.started_at);
    previous = at;
    rows.push({ ms: at, kind: "operation.step_created", step: step.id, state: null });
    if (step.kind === "execution" && step.started_at !== null) {
      rows.push({ ms: at, kind: "operation.execution", step: step.id, state: "dispatched" });
    }
    last = Math.max(last, at);
    if (step.ended_at !== null) {
      const end = sec(step.ended_at);
      rows.push({ ms: end, kind: "operation.step_finished", step: step.id, state: null });
      last = Math.max(last, end);
    }
  }
  const finalStatus = trace.operation.status;
  if (finalStatus !== "running") {
    const endedAt = trace.operation.ended_at;
    rows.push({
      ms: endedAt === null ? last : Math.max(last, sec(endedAt)),
      kind: "operation.status",
      step: null,
      state: finalStatus,
    });
  }
  const ordered = rows.map((r, i) => ({ r, i })).sort((a, b) => a.r.ms - b.r.ms || a.i - b.i);
  return historyOf(ordered.map(({ r }, i) => ev(i + 1, iso(r.ms), r.kind, r.step, r.state)));
}

/** Freeze a value and everything inside it, so a test proves a function does not mutate it. */
export function deepFreeze<T>(value: T): T {
  if (typeof value === "object" && value !== null && !Object.isFrozen(value)) {
    Object.freeze(value);
    for (const inner of Object.values(value)) {
      deepFreeze(inner);
    }
  }
  return value;
}

/** A value that must exist; fails the test when it does not. */
export function must<T>(value: T | undefined): T {
  if (value === undefined) {
    throw new Error("expected a value");
  }
  return value;
}
