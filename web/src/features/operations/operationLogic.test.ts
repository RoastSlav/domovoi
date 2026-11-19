import { describe, expect, it } from "vitest";
import type { OperationTrace, TraceStep } from "../../api/operationTrace";
import type { Action, Execution, OperationSummary } from "../../api/types";
import failedVerification from "../../test/fixtures/operation-trace/failed-verification-new-cycle.json";
import outcomeUnknown from "../../test/fixtures/operation-trace/outcome-unknown-paused.json";
import { operationBanner } from "./banner";
import { formatBudget, fromApiLimit, limitFor, toApiLimit } from "./budget";
import { mergeRows } from "./operationRows";
import {
  awaitingExecutions,
  buildTree,
  byCycle,
  connections,
  planTextFor,
  windowSteps,
} from "./steps";

const names = new Map([["host-nas", "nas"]]);

function step(id: string, over: Partial<TraceStep> = {}): TraceStep {
  return {
    id,
    kind: "note",
    phase: "x",
    cycle: 0,
    outcome: "succeeded",
    refs: [],
    ...over,
  };
}

function summary(id: string, updated_at: string): OperationSummary {
  return {
    id,
    kind: "investigate",
    goal: `goal ${id}`,
    status: "running",
    waiting_for: [],
    cycle: 0,
    incidents: [],
    hosts: ["host-nas"],
    created_at: updated_at,
    updated_at,
  };
}

function action(id: string, updated_at: string): Action {
  return {
    id,
    skill: "restart_container",
    host_id: "host-nas",
    incident_id: `inc-${id}`,
    status: "proposed",
    legacy: true,
    updated_at,
    created_at: updated_at,
  } as unknown as Action;
}

function execution(over: Partial<Execution> = {}): Execution {
  return { step: "s", host_id: "host-nas", state: "awaiting_approval", ...over } as Execution;
}

describe("merging rows", () => {
  it("merge_lists_operations_and_legacy_actions_newest_first", () => {
    const rows = mergeRows({
      operations: [summary("o1", "2026-01-03T00:00:00Z"), summary("o2", "2026-01-01T00:00:00Z")],
      legacy: [action("a1", "2026-01-02T00:00:00Z"), action("a2", "2026-01-03T00:00:00Z")],
      hostNames: names,
      moreOperations: false,
      moreLegacy: false,
    });
    expect(rows.map((row) => row.id)).toEqual(["o1", "a2", "a1", "o2"]);
  });

  it("merge_hides_rows_older_than_the_oldest_loaded_row_of_a_source_with_more_pages", () => {
    const rows = mergeRows({
      operations: [summary("o1", "2026-01-05T00:00:00Z"), summary("o2", "2026-01-03T00:00:00Z")],
      legacy: [action("a1", "2026-01-04T00:00:00Z"), action("a2", "2026-01-01T00:00:00Z")],
      hostNames: names,
      moreOperations: true,
      moreLegacy: false,
    });
    expect(rows.map((row) => row.id)).toEqual(["o1", "a1", "o2"]);
  });

  it("merge_shows_everything_when_both_sources_are_complete", () => {
    const rows = mergeRows({
      operations: [summary("o1", "2026-01-05T00:00:00Z")],
      legacy: [action("a2", "2026-01-01T00:00:00Z")],
      hostNames: names,
      moreOperations: false,
      moreLegacy: false,
    });
    expect(rows).toHaveLength(2);
  });

  it("merge_marks_legacy_rows_and_points_them_at_their_incident", () => {
    const [row] = mergeRows({
      operations: [],
      legacy: [action("a1", "2026-01-01T00:00:00Z")],
      hostNames: names,
      moreOperations: false,
      moreLegacy: false,
    });
    expect(row).toMatchObject({
      kind: "legacy",
      href: "/incidents/inc-a1",
      goal: "restart_container on nas",
    });
  });
});

describe("banner", () => {
  const trace = outcomeUnknown as unknown as OperationTrace;
  const op = (status: string, pause_reason?: OperationSummary["pause_reason"]) => ({
    status,
    hosts: ["host-nas"],
    pause_reason: pause_reason ?? null,
  });

  it.each([
    ["outcome_unknown", { type: "outcome_unknown", step: "s" }, "outcome_unknown"],
    ["budget", { type: "budget", exhausted: "model_turns" }, "budget"],
    ["error", { type: "error", message: "boom" }, "error"],
    ["manual", { type: "manual" }, "manual"],
    ["global_pause", { type: "global_pause" }, "paused"],
    ["mystery", { type: "mystery" }, "paused"],
  ])("banner_priority_for_pause_%s", (_name, reason, key) => {
    expect(operationBanner(op("paused", reason), trace.steps, [], names)?.key).toBe(key);
  });

  it.each([
    ["waiting_approval", "waiting_approval"],
    ["waiting_user", "waiting_user"],
    ["waiting_agent", "waiting_agent"],
    ["failed", "failed"],
    ["escalated", "escalated"],
  ])("banner_for_%s", (status, key) => {
    expect(operationBanner(op(status), [], [], names)?.key).toBe(key);
  });

  it("no_banner_for_queued_running_waiting_timer_succeeded_cancelled", () => {
    for (const status of ["queued", "running", "waiting_timer", "succeeded", "cancelled"]) {
      expect(operationBanner(op(status), [], [], names)).toBeNull();
    }
  });

  it("banner_for_waiting_approval_counts_the_pending_actions", () => {
    const banner = operationBanner(op("waiting_approval"), [], [execution(), execution()], names);
    expect(banner?.body).toContain("2 actions");
  });

  it("budget_banner_names_the_exhausted_limit_in_lower_case", () => {
    const banner = operationBanner(
      op("paused", { type: "budget", exhausted: "model_turns" }),
      [],
      [],
      names,
    );
    expect(banner?.title).toBe("Paused: the model turns limit is spent");
    expect(banner?.actions).toEqual(["raise_limit", "cancel"]);
  });

  it("waiting_agent_banner_names_the_hosts", () => {
    expect(operationBanner(op("waiting_agent"), [], [], names)?.title).toBe("Waiting for nas");
  });
});

describe("steps", () => {
  const trace = failedVerification as unknown as OperationTrace;

  it("steps_group_by_cycle_in_ordinal_order", () => {
    const groups = byCycle(trace.steps);
    expect(groups.map((group) => group.cycle)).toEqual([0, 1]);
    expect(groups[0]?.steps.map((s) => s.id)).toEqual(
      trace.steps.filter((s) => s.cycle === 0).map((s) => s.id),
    );
  });

  it("tree_nests_children_under_parents_and_treats_missing_parents_as_roots", () => {
    const tree = buildTree([
      step("a"),
      step("b", { parent: "a" }),
      step("c", { parent: "missing" }),
    ]);
    expect(tree.map((node) => node.step.id)).toEqual(["a", "c"]);
    expect(tree[0]?.children.map((node) => node.step.id)).toEqual(["b"]);
  });

  it("window_keeps_open_unknown_and_selected_steps", () => {
    const steps = Array.from({ length: 300 }, (_, i) => step(`s${i}`));
    steps[0] = step("s0", { outcome: "running" });
    steps[1] = step("s1", { outcome: "outcome_unknown" });
    const shown = windowSteps(steps, { limit: 200, showAll: false, keep: ["s2"] }).map((s) => s.id);
    expect(shown).toHaveLength(203);
    expect(shown.slice(0, 3)).toEqual(["s0", "s1", "s2"]);
    expect(windowSteps(steps, { limit: 200, showAll: true })).toHaveLength(300);
  });

  it("connections_list_both_directions_with_edge_labels", () => {
    const list = connections(trace, "01900000-0000-7000-8000-000000030003");
    expect(list.find((c) => c.edge.kind === "depends_on")?.label).toBe("Depends on");
    expect(list.find((c) => c.edge.kind === "verifies")?.label).toBe("Verified by");
  });

  it("plan_text_is_the_plan_step_linked_by_an_edge_or_the_latest_before_it_in_the_cycle", () => {
    const small: OperationTrace = {
      ...trace,
      steps: [
        step("p1", { kind: "plan", summary: "first" }),
        step("p2", { kind: "plan", summary: "second" }),
        step("e1", { kind: "execution" }),
        step("e2", { kind: "execution" }),
        step("e3", { kind: "execution", cycle: 5 }),
      ],
      edges: [{ from: "e2", to: "p1", kind: "caused_by" }],
    };
    expect(planTextFor(small, "e2")).toBe("first");
    expect(planTextFor(small, "e1")).toBe("second");
    expect(planTextFor(small, "e3")).toBeUndefined();
  });

  it("awaiting_executions_are_those_in_the_awaiting_state", () => {
    expect(awaitingExecutions([execution(), execution({ state: "succeeded" })])).toHaveLength(1);
  });
});

describe("budget", () => {
  const trace = failedVerification as unknown as OperationTrace;

  it("budget_units_convert_both_ways", () => {
    expect(toApiLimit("wall_time", 5)).toBe(300);
    expect(fromApiLimit("wall_time", 300)).toBe(5);
    expect(toApiLimit("cost", 1.5)).toBe(1_500_000);
    expect(fromApiLimit("cost", 1_500_000)).toBe(1.5);
    expect(toApiLimit("tokens", 10)).toBe(10);
    expect(limitFor("cycles", trace).used).toBe(trace.operation.cycle);
  });

  it("format_budget_says_no_limit_for_null", () => {
    expect(formatBudget("model_turns", 3, null)).toBe("3 (no limit)");
    expect(formatBudget("model_turns", 3, 10)).toBe("3 of 10");
    expect(formatBudget("tokens", 1500, 10000)).toBe("1,500 of 10,000");
  });
});
