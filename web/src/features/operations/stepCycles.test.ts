import { describe, expect, it } from "vitest";
import type { TraceStep } from "../../api/operationTrace";
import { limitProblem } from "./budget";
import { buildTree, type StepNode } from "./steps";

function step(id: string, parent?: string): TraceStep {
  return {
    id,
    kind: "note",
    phase: "x",
    cycle: 0,
    outcome: "succeeded",
    refs: [],
    parent,
  } as TraceStep;
}

function ids(nodes: StepNode[]): string[] {
  return nodes.flatMap((node) => [node.step.id, ...ids(node.children)]);
}

describe("tree with parent loops", () => {
  it("a_two_step_parent_loop_keeps_both_steps_with_the_closing_step_as_root", () => {
    const tree = buildTree([step("a", "b"), step("b", "a")]);
    expect(tree.map((node) => node.step.id)).toEqual(["b"]);
    expect(ids(tree).sort()).toEqual(["a", "b"]);
  });

  it("a_three_step_parent_loop_keeps_every_step", () => {
    const tree = buildTree([step("a", "c"), step("b", "a"), step("c", "b")]);
    expect(ids(tree).sort()).toEqual(["a", "b", "c"]);
  });

  it("a_loop_beside_a_valid_root_changes_nothing_for_the_valid_root", () => {
    const tree = buildTree([step("r"), step("x", "y"), step("y", "x"), step("k", "r")]);
    expect(tree.map((node) => node.step.id)).toEqual(["r", "y"]);
    expect(ids(tree).sort()).toEqual(["k", "r", "x", "y"]);
  });
});

describe("limitProblem", () => {
  it("whole_number_budgets_refuse_fractions_instead_of_rounding_them", () => {
    for (const kind of [
      "model_turns",
      "evidence_requests",
      "executions",
      "cycles",
      "tokens",
    ] as const) {
      expect(limitProblem(kind, 10.8)).toBe("Enter a whole number.");
      expect(limitProblem(kind, 11)).toBeNull();
    }
    expect(limitProblem("wall_time", 1.5)).toBeNull();
    expect(limitProblem("cost", 0.25)).toBeNull();
  });
});
