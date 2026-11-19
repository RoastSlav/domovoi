import type { OperationTrace, TraceEdge, TraceStep } from "../../api/operationTrace";
import type { Execution } from "../../api/types";
import { reverseEdgeLabel } from "../../design/operationText";
import { edgeKindMeta } from "../../design/tone";

/** Steps of one cycle, in trace (ordinal) order. */
export interface CycleGroup {
  cycle: number;
  steps: TraceStep[];
}

/** Groups steps by cycle, cycles ascending, each in the order the trace lists them. */
export function byCycle(steps: readonly TraceStep[]): CycleGroup[] {
  const groups = new Map<number, TraceStep[]>();
  for (const step of steps) {
    const list = groups.get(step.cycle);
    if (list) {
      list.push(step);
    } else {
      groups.set(step.cycle, [step]);
    }
  }
  return [...groups.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([cycle, list]) => ({ cycle, steps: list }));
}

/** A step with the steps grouped under it. */
export interface StepNode {
  step: TraceStep;
  children: StepNode[];
}

/** Nests steps by `parent`; a step whose parent is not in the list is a root. */
export function buildTree(steps: readonly TraceStep[]): StepNode[] {
  const nodes = new Map<string, StepNode>(steps.map((step) => [step.id, { step, children: [] }]));
  const roots: StepNode[] = [];
  for (const step of steps) {
    const node = nodes.get(step.id);
    if (!node) {
      continue;
    }
    const parent = step.parent ? nodes.get(step.parent) : undefined;
    if (parent && parent !== node) {
      parent.children.push(node);
    } else {
      roots.push(node);
    }
  }
  return roots;
}

/** What `windowSteps` keeps. */
export interface WindowOptions {
  /** How many of the newest steps to show. */
  limit: number;
  /** Show everything. */
  showAll: boolean;
  /** Ids that stay visible whatever their age (the selected step). */
  keep?: readonly string[];
}

const OPEN_OUTCOMES = new Set(["pending", "running", "outcome_unknown"]);

/** The newest `limit` steps plus every open, result-unknown and kept one; all of them with `showAll`. */
export function windowSteps(steps: readonly TraceStep[], options: WindowOptions): TraceStep[] {
  if (options.showAll || steps.length <= options.limit) {
    return [...steps];
  }
  const keep = new Set(options.keep ?? []);
  const firstNewest = steps.length - options.limit;
  return steps.filter(
    (step, index) => index >= firstNewest || OPEN_OUTCOMES.has(step.outcome) || keep.has(step.id),
  );
}

/** The step `stepId` is a retry of, if any. */
export function retryTarget(trace: OperationTrace, stepId: string): string | undefined {
  return trace.edges.find((edge) => edge.from === stepId && edge.kind === "retry_of")?.to;
}

/** One link from a step to another, labelled from the step's point of view. */
export interface Connection {
  edge: TraceEdge;
  label: string;
  other: TraceStep;
}

/** Edges leaving and entering `stepId`: outgoing with the edge's own words, incoming reversed. */
export function connections(trace: OperationTrace, stepId: string): Connection[] {
  const byId = new Map(trace.steps.map((step) => [step.id, step]));
  const result: Connection[] = [];
  for (const edge of trace.edges) {
    if (edge.from === stepId) {
      const other = byId.get(edge.to);
      if (other) {
        result.push({ edge, label: edgeKindMeta(edge.kind).label, other });
      }
    } else if (edge.to === stepId) {
      const other = byId.get(edge.from);
      if (other) {
        result.push({ edge, label: reverseEdgeLabel(edge.kind), other });
      }
    }
  }
  return result;
}

/** The summary of the plan an execution follows: by edge, else the latest earlier plan in its cycle. */
export function planTextFor(trace: OperationTrace, executionStepId: string): string | undefined {
  const byId = new Map(trace.steps.map((step) => [step.id, step]));
  for (const edge of trace.edges) {
    if (
      edge.from === executionStepId &&
      (edge.kind === "caused_by" || edge.kind === "depends_on")
    ) {
      const target = byId.get(edge.to);
      if (target?.kind === "plan") {
        return target.summary ?? undefined;
      }
    }
  }
  const index = trace.steps.findIndex((step) => step.id === executionStepId);
  const execution = trace.steps[index];
  if (!execution) {
    return undefined;
  }
  for (let i = index - 1; i >= 0; i -= 1) {
    const step = trace.steps[i];
    if (step?.kind === "plan" && step.cycle === execution.cycle) {
      return step.summary ?? undefined;
    }
  }
  return undefined;
}

/** Question steps that still wait for an answer. */
export function pendingQuestions(trace: OperationTrace): TraceStep[] {
  const answered = new Set(
    trace.edges.filter((edge) => edge.kind === "answers").map((edge) => edge.to),
  );
  return trace.steps.filter(
    (step) => step.kind === "question" && step.outcome === "pending" && !answered.has(step.id),
  );
}

/** Executions whose approval is open. */
export function awaitingExecutions(executions: readonly Execution[]): Execution[] {
  return executions.filter((execution) => execution.state === "awaiting_approval");
}
