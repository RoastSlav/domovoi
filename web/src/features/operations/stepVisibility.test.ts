import { describe, expect, it } from "vitest";
import type { TraceStep } from "../../api/operationTrace";
import { PROFILE_IDS, type Category } from "../../design/visibility";
import {
  groupSteps,
  reasonCategory,
  stepCategory,
  treeCategory,
  type OutlineItem,
} from "./stepVisibility";

const KINDS: [string, Category][] = [
  ["approval", "P0"],
  ["execution", "P0"],
  ["verification", "P0"],
  ["question", "P0"],
  ["assessment", "P1"],
  ["plan", "P1"],
  ["policy_decision", "P1"],
  ["notification", "P1"],
  ["note", "P1"],
  ["evidence_request", "P2"],
  ["reasoning_turn", "P2"],
];
const PROBLEMS = ["failed", "refused", "not_run", "unavailable", "outcome_unknown"];
const QUIET = ["running", "pending", "skipped", "cancelled"];

function step(id: string, kind: string, outcome: string, parent: string | null = null): TraceStep {
  return {
    id,
    kind,
    outcome,
    parent,
    cycle: 0,
    phase: "diagnose",
    refs: [],
    started_at: null,
    ended_at: null,
    summary: null,
  } as unknown as TraceStep;
}

/** The shape of a grouping: "single" for a row, the size for a group. */
function shape<T>(items: OutlineItem<T>[]): (number | "single")[] {
  return items.map((entry) => (entry.type === "item" ? "single" : entry.items.length));
}

describe("stepCategory", () => {
  it.each(KINDS)("kind_decides_the_base_category (%s is %s)", (kind, category) => {
    expect(stepCategory(kind, "succeeded")).toBe(category);
  });

  it.each(KINDS.flatMap(([kind]) => PROBLEMS.map((outcome) => [kind, outcome])))(
    "a_problem_outcome_makes_any_kind_p0 (%s, %s)",
    (kind, outcome) => {
      expect(stepCategory(kind as string, outcome as string)).toBe("P0");
    },
  );

  it.each(KINDS.flatMap(([kind, category]) => QUIET.map((outcome) => [kind, outcome, category])))(
    "running_pending_skipped_and_cancelled_keep_the_base_category (%s, %s is %s)",
    (kind, outcome, category) => {
      expect(stepCategory(kind as string, outcome as string)).toBe(category);
    },
  );

  it("unknown_kind_is_p1_and_unknown_outcome_is_p0", () => {
    expect(stepCategory("something_new", "succeeded")).toBe("P1");
    expect(stepCategory("something_new", "running")).toBe("P1");
    expect(stepCategory("evidence_request", "something_new")).toBe("P0");
    expect(stepCategory("something_new", "something_new")).toBe("P0");
    expect(stepCategory("note", "")).toBe("P0");
  });
});

describe("reasonCategory", () => {
  it("reason_category_is_p0_for_problem_and_unknown_outcomes_else_p1", () => {
    for (const outcome of PROBLEMS) {
      expect(reasonCategory(outcome)).toBe("P0");
    }
    expect(reasonCategory("something_new")).toBe("P0");
    for (const outcome of ["succeeded", ...QUIET]) {
      expect(reasonCategory(outcome)).toBe("P1");
    }
  });
});

describe("treeCategory", () => {
  it("tree_category_is_the_most_important_of_the_step_and_its_descendants", () => {
    const steps = [
      step("parent", "reasoning_turn", "succeeded"),
      step("child", "evidence_request", "succeeded", "parent"),
      step("grandchild", "execution", "succeeded", "child"),
    ];
    expect(treeCategory(steps, "parent")).toBe("P0");
    expect(treeCategory(steps, "child")).toBe("P0");
    expect(treeCategory(steps, "grandchild")).toBe("P0");
  });

  it("a_parent_with_only_technical_children_stays_technical", () => {
    const steps = [
      step("parent", "reasoning_turn", "succeeded"),
      step("a", "evidence_request", "succeeded", "parent"),
      step("b", "reasoning_turn", "running", "parent"),
    ];
    expect(treeCategory(steps, "parent")).toBe("P2");
    expect(treeCategory(steps, "a")).toBe("P2");
  });

  it("a_problem_in_a_deep_descendant_raises_every_ancestor", () => {
    const steps = [
      step("root", "evidence_request", "succeeded"),
      step("mid", "reasoning_turn", "succeeded", "root"),
      step("leaf", "evidence_request", "failed", "mid"),
      step("other", "evidence_request", "succeeded"),
    ];
    expect(treeCategory(steps, "root")).toBe("P0");
    expect(treeCategory(steps, "mid")).toBe("P0");
    expect(treeCategory(steps, "other")).toBe("P2");
  });

  it("a_step_whose_parent_is_missing_is_a_root_of_its_own", () => {
    const steps = [step("orphan", "evidence_request", "succeeded", "gone")];
    expect(treeCategory(steps, "orphan")).toBe("P2");
  });

  it("a_parent_loop_does_not_hang_and_still_reports_the_best_category", () => {
    const steps = [
      step("a", "reasoning_turn", "succeeded", "b"),
      step("b", "assessment", "succeeded", "a"),
    ];
    expect(treeCategory(steps, "a")).toBe("P1");
  });
});

describe("groupSteps", () => {
  const categoryOf = (category: Category) => category;

  it("group_steps_groups_consecutive_non_shown_items_and_leaves_the_rest_single", () => {
    const items: Category[] = ["P0", "P2", "P2", "P1", "P2", "P0"];
    expect(shape(groupSteps(items, "standard", categoryOf))).toEqual([
      "single",
      2,
      "single",
      1,
      "single",
    ]);
    expect(shape(groupSteps(items, "essential", categoryOf))).toEqual([
      "single",
      2,
      "single",
      1,
      "single",
    ]);
  });

  it("group_steps_never_groups_anything_in_detailed", () => {
    const items: Category[] = ["P0", "P2", "P2", "P3", "P1", "P2"];
    const result = groupSteps(items, "detailed", categoryOf);
    expect(result.every((entry) => entry.type === "item")).toBe(true);
    expect(result).toHaveLength(items.length);
  });

  it("group_steps_groups_p3_items_too_outside_detailed", () => {
    const items: Category[] = ["P2", "P3", "P2"];
    expect(shape(groupSteps(items, "standard", categoryOf))).toEqual([3]);
  });

  it("group_steps_never_puts_a_p0_or_p1_item_in_a_group", () => {
    const all = KINDS.flatMap(([kind]) =>
      [...PROBLEMS, ...QUIET, "succeeded", "something_new"].map((outcome) => ({ kind, outcome })),
    );
    // Every adjacent pair, so a technical neighbour is next to every possible item.
    const technical = { kind: "evidence_request", outcome: "succeeded" };
    const sequence = all.flatMap((item) => [technical, item, technical]);
    for (const profile of PROFILE_IDS) {
      const result = groupSteps(sequence, profile, (item) => stepCategory(item.kind, item.outcome));
      for (const entry of result) {
        if (entry.type === "group") {
          for (const item of entry.items) {
            expect(["P0", "P1"]).not.toContain(stepCategory(item.kind, item.outcome));
          }
        }
      }
    }
  });

  it("group_steps_preserves_order_and_every_item_exactly_once", () => {
    const all = KINDS.flatMap(([kind]) =>
      [...PROBLEMS, ...QUIET, "succeeded"].map((outcome) => ({ kind, outcome })),
    );
    const sequence = all.map((item, index) => ({ ...item, index }));
    for (const profile of PROFILE_IDS) {
      const result = groupSteps(sequence, profile, (item) => stepCategory(item.kind, item.outcome));
      const flat = result.flatMap((entry) => (entry.type === "item" ? [entry.item] : entry.items));
      expect(flat.map((item) => item.index)).toEqual(sequence.map((item) => item.index));
    }
  });

  it("group_steps_of_nothing_is_nothing", () => {
    expect(groupSteps([], "essential", categoryOf)).toEqual([]);
  });
});
