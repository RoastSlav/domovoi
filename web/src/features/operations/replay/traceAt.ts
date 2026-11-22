import type { OperationTrace, TraceStep } from "../../../api/operationTrace";
import { floorSecond, type Frame } from "./frames";
import type { HistoryEvent, OperationHistory } from "../../../api/types";

/** First time (whole second) per step of the events of one kind, optionally filtered by state. */
function firstSecondPerStep(
  events: readonly HistoryEvent[],
  kind: string,
  state?: string,
): Map<string, number> {
  const out = new Map<string, number>();
  for (const e of events) {
    if (e.kind === kind && e.step != null && (state === undefined || e.state === state)) {
      const at = floorSecond(e.at);
      const known = out.get(e.step);
      if (at !== null && (known === undefined || at < known)) {
        out.set(e.step, at);
      }
    }
  }
  return out;
}

/**
 * The trace as it stood at the end of the second `atMs`, rebuilt from the final trace and the
 * history. It never invents a step, an outcome or an edge, and never mutates `final`.
 */
export function traceAt(input: {
  final: OperationTrace;
  frames: readonly Frame[];
  history: OperationHistory;
  atMs: number;
}): OperationTrace {
  const { final, history, frames, atMs: t } = input;
  const createdAt = firstSecondPerStep(history.events, "operation.step_created");
  const dispatchedAt = firstSecondPerStep(history.events, "operation.execution", "dispatched");
  const ordinals = final.steps.flatMap((s, i) => (createdAt.has(s.id) ? [i] : []));
  const loOrd = ordinals[0];
  const pruned = history.incomplete_reason === "pruned" || history.incomplete_reason === "both";

  const exists = (step: TraceStep, ordinal: number): boolean => {
    const created = createdAt.get(step.id);
    if (created !== undefined) {
      return created <= t;
    }
    if (loOrd === undefined) {
      return pruned;
    }
    return ordinal < loOrd;
  };
  const finalMs = (step: TraceStep): number | null => {
    if (step.outcome === "pending" || step.outcome === "running") {
      return null;
    }
    return floorSecond(step.ended_at) ?? Number.POSITIVE_INFINITY;
  };
  const outcomeAt = (step: TraceStep): string => {
    const f = finalMs(step);
    if (f !== null && f <= t) {
      return step.outcome;
    }
    const starts = [floorSecond(step.started_at), dispatchedAt.get(step.id)];
    const runFrom = Math.min(...starts.filter((v): v is number => typeof v === "number"));
    if (step.outcome !== "pending" && Number.isFinite(runFrom) && runFrom <= t) {
      return "running";
    }
    return "pending";
  };

  const steps: TraceStep[] = [];
  final.steps.forEach((step, ordinal) => {
    if (!exists(step, ordinal)) {
      return;
    }
    const outcome = outcomeAt(step);
    const f = finalMs(step);
    const startedMs = floorSecond(step.started_at);
    steps.push({
      ...step,
      outcome,
      started_at: startedMs !== null && startedMs <= t ? (step.started_at ?? null) : null,
      ended_at: outcome === step.outcome && f !== null && f <= t ? (step.ended_at ?? null) : null,
    });
  });
  const ids = new Set(steps.map((s) => s.id));
  const edges = final.edges.filter((e) => ids.has(e.from) && ids.has(e.to));

  let status = final.operation.status;
  let statusSeen = false;
  let createdSeen = false;
  for (const e of history.events) {
    const at = floorSecond(e.at);
    if (at === null || at > t) {
      continue;
    }
    if (e.kind === "operation.created") {
      createdSeen = true;
    }
    if (e.kind === "operation.status" && e.state != null) {
      status = e.state;
      statusSeen = true;
    }
  }
  if (!statusSeen && createdSeen) {
    status = "queued";
  }
  const seq = frames.reduce((max, f) => (f.atMs <= t ? Math.max(max, f.seq) : max), 0);
  return { ...final, as_of_seq: seq, operation: { ...final.operation, status }, steps, edges };
}
