import type { OperationTrace, TraceStep } from "../../../api/operationTrace";
import { EDGE_KINDS, STEP_KINDS, STEP_OUTCOMES, WAIT_KINDS } from "../../../api/operationTrace";
import failedVerification from "./failed-verification-new-cycle.json";
import outcomeUnknown from "./outcome-unknown-paused.json";
import questionAndBudget from "./question-and-budget-pause.json";
import verifiedSuccess from "./verified-success.json";
import waitingApproval from "./waiting-approval.json";

const FIXTURES: [string, OperationTrace][] = [
  ["waiting-approval", waitingApproval satisfies OperationTrace],
  ["verified-success", verifiedSuccess satisfies OperationTrace],
  ["failed-verification-new-cycle", failedVerification satisfies OperationTrace],
  ["outcome-unknown-paused", outcomeUnknown satisfies OperationTrace],
  ["question-and-budget-pause", questionAndBudget satisfies OperationTrace],
];

const FORBIDDEN_KEYS = ["output", "intent", "plan_hash"];

/** Every object key in a JSON value, at any depth. */
function allKeys(value: unknown, out: string[] = []): string[] {
  if (Array.isArray(value)) {
    for (const item of value) allKeys(item, out);
  } else if (value !== null && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      out.push(key);
      allKeys(child, out);
    }
  }
  return out;
}

/** True when `part` is a subsequence of `whole`, keeping order. */
function isSubsequence<T>(part: readonly T[], whole: readonly T[]): boolean {
  let at = 0;
  for (const item of part) {
    while (at < whole.length && whole[at] !== item) at += 1;
    if (at === whole.length) return false;
    at += 1;
  }
  return true;
}

function stepIds(trace: OperationTrace): Set<string> {
  return new Set(trace.steps.map((step) => step.id));
}

function indexOf(trace: OperationTrace): Map<string, number> {
  return new Map(trace.steps.map((step, index) => [step.id, index]));
}

describe.each(FIXTURES)("operation trace fixture %s", (_name, trace) => {
  it("is schema version 1", () => {
    expect(trace.schema_version).toBe(1);
  });

  it("has unique step ids", () => {
    const ids = trace.steps.map((step) => step.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("names an existing step for every parent", () => {
    const ids = stepIds(trace);
    for (const step of trace.steps) {
      if (step.parent !== null && step.parent !== undefined) {
        expect(ids.has(step.parent)).toBe(true);
      }
    }
  });

  it("names existing steps at both ends of every edge", () => {
    const ids = stepIds(trace);
    for (const edge of trace.edges) {
      expect(ids.has(edge.from)).toBe(true);
      expect(ids.has(edge.to)).toBe(true);
    }
  });

  it("lists every edge's prerequisite before its dependent step", () => {
    const at = indexOf(trace);
    for (const edge of trace.edges) {
      expect(at.get(edge.from) ?? -1).toBeGreaterThan(at.get(edge.to) ?? Number.MAX_SAFE_INTEGER);
    }
  });

  it("links only to executions, and each link names a step", () => {
    const ids = stepIds(trace);
    for (const step of trace.steps) {
      for (const ref of step.refs) {
        expect(ref.kind).toBe("execution");
        expect(ids.has(ref.id)).toBe(true);
      }
    }
  });

  it("carries no output, intent or plan hash anywhere", () => {
    const keys = allKeys(trace);
    for (const forbidden of FORBIDDEN_KEYS) {
      expect(keys).not.toContain(forbidden);
    }
  });

  it("lists waiting kinds as a subsequence of the priority order", () => {
    expect(isSubsequence(trace.operation.waiting_for, WAIT_KINDS)).toBe(true);
  });
});

describe("operation trace fixtures together", () => {
  const steps: TraceStep[] = FIXTURES.flatMap(([, trace]) => trace.steps);
  const edges = FIXTURES.flatMap(([, trace]) => trace.edges);

  it("cover every step kind", () => {
    const seen = new Set(steps.map((step) => step.kind));
    expect(STEP_KINDS).toHaveLength(11);
    for (const kind of STEP_KINDS) expect(seen).toContain(kind);
  });

  it("cover every step outcome", () => {
    const seen = new Set(steps.map((step) => step.outcome));
    expect(STEP_OUTCOMES).toHaveLength(10);
    for (const outcome of STEP_OUTCOMES) expect(seen).toContain(outcome);
  });

  it("cover every edge kind", () => {
    const seen = new Set(edges.map((edge) => edge.kind));
    expect(EDGE_KINDS).toHaveLength(5);
    for (const kind of EDGE_KINDS) expect(seen).toContain(kind);
  });
});
