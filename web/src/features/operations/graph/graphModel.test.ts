import { describe, expect, it } from "vitest";
import type { OperationTrace } from "../../../api/operationTrace";
import type { GraphModel } from "../../../components/graph/types";
import type { ProfileId } from "../../../design/visibility";
import { makeTrace } from "../../../test/syntheticTrace";
import { TRACE_FIXTURES } from "../../../test/operationFixtures";
import { buildGraphModel, groupId, type BuildInput } from "./graphModel";

const FIXTURES: [string, OperationTrace][] = [
  ["waiting-approval", TRACE_FIXTURES["waiting-approval"]],
  ["verified-success", TRACE_FIXTURES["verified-success"]],
  ["failed-verification-new-cycle", TRACE_FIXTURES["failed-verification-new-cycle"]],
  ["outcome-unknown-paused", TRACE_FIXTURES["outcome-unknown-paused"]],
  ["question-and-budget-pause", TRACE_FIXTURES["question-and-budget-pause"]],
];

type StepSpec = [id: string, kind: string, outcome?: string, cycle?: number];

/** A small trace: ordinal order is the list order. */
function trace(steps: StepSpec[], edges: [string, string, string][] = []): OperationTrace {
  return {
    ...TRACE_FIXTURES["verified-success"],
    steps: steps.map(([id, kind, outcome = "succeeded", cycle = 0]) => ({
      id,
      kind,
      outcome,
      cycle,
      parent: null,
      phase: "diagnose",
      refs: [],
      summary: `about ${id}`,
    })),
    edges: edges.map(([from, to, kind]) => ({ from, to, kind })),
  };
}

function build(t: OperationTrace, profile: ProfileId, extra: Partial<BuildInput> = {}) {
  return buildGraphModel({
    trace: t,
    profile,
    expanded: new Set(),
    selected: null,
    showAll: false,
    ...extra,
  });
}

const groups = (m: GraphModel) => m.nodes.filter((n) => n.kind === "group");
const ids = (m: GraphModel) => m.nodes.map((n) => n.id);

/** A trace of technical steps around a plan and an approval. */
const TECHNICAL = trace(
  [
    ["p", "plan"],
    ["e1", "evidence_request"],
    ["e2", "evidence_request"],
    ["e3", "evidence_request"],
    ["r1", "reasoning_turn"],
    ["a", "approval"],
  ],
  [
    ["e1", "p", "depends_on"],
    ["e2", "e1", "depends_on"],
    ["r1", "e3", "depends_on"],
    ["a", "r1", "depends_on"],
  ],
);

describe.each(FIXTURES)("graph of %s", (_name, fixture) => {
  it("detailed_profile_draws_every_step_and_every_edge_of_each_fixture", () => {
    const { model } = build(fixture, "detailed");
    const distinct = new Set(fixture.edges.map((e) => `${e.from}>${e.to}:${e.kind}`));
    expect(model.nodes).toHaveLength(fixture.steps.length);
    expect(model.edges).toHaveLength(distinct.size);
    expect(groups(model)).toEqual([]);
    for (const n of model.nodes) {
      expect(n.title).not.toBe("");
      expect(n.status.label).not.toBe("");
    }
    expect(build(fixture, "detailed").model.layoutKey).toBe(model.layoutKey);
  });

  it("a_group_never_holds_a_p0_or_p1_step", () => {
    const problems = ["failed", "refused", "not_run", "unavailable", "outcome_unknown"];
    const byId = new Map(fixture.steps.map((s) => [s.id, s]));
    for (const profile of ["standard", "essential"] as const) {
      for (const g of groups(build(fixture, profile).model)) {
        for (const member of g.members) {
          const step = byId.get(member);
          expect(["evidence_request", "reasoning_turn"]).toContain(step?.kind);
          expect(problems).not.toContain(step?.outcome);
        }
      }
    }
  });
});

describe("graph model", () => {
  it("status_only_change_keeps_the_layout_key", () => {
    const before = trace(
      [
        ["a", "plan", "running"],
        ["b", "plan"],
      ],
      [["b", "a", "depends_on"]],
    );
    const after = trace(
      [
        ["a", "plan", "succeeded"],
        ["b", "plan"],
      ],
      [["b", "a", "depends_on"]],
    );
    const x = build(before, "detailed").model;
    const y = build(after, "detailed").model;
    expect(y.layoutKey).toBe(x.layoutKey);
    expect(x.nodes[0]?.status.value).toBe("running");
    expect(y.nodes[0]?.status.value).toBe("succeeded");
  });

  it("a_new_step_or_edge_changes_the_layout_key", () => {
    const base = trace([
      ["a", "plan"],
      ["b", "plan"],
    ]);
    const key = (t: OperationTrace) => build(t, "detailed").model.layoutKey;
    const withStep = trace([
      ["a", "plan"],
      ["b", "plan"],
      ["c", "plan"],
    ]);
    const withEdge = trace(
      [
        ["a", "plan"],
        ["b", "plan"],
      ],
      [["b", "a", "depends_on"]],
    );
    expect(key(withStep)).not.toBe(key(base));
    expect(key(withEdge)).not.toBe(key(base));
  });

  it("standard_profile_folds_runs_of_technical_steps_into_one_group", () => {
    const { model, foldedGroups, groupOf } = build(TECHNICAL, "standard");
    expect(ids(model)).toEqual(["p", "group:e1", "a"]);
    const group = groups(model)[0];
    expect(group?.members).toEqual(["e1", "e2", "e3", "r1"]);
    expect(group?.title).toBe("4 technical steps");
    expect(group?.summary).toBe("Evidence ×3, Model turn");
    expect(group?.status).toMatchObject({ label: "Folded", tone: "neutral", value: "" });
    expect(group?.ordinal).toBe(1);
    expect(foldedGroups).toBe(1);
    expect(groupOf.get("r1")).toBe("group:e1");
    expect(groupId("e1")).toBe("group:e1");
  });

  it("a_single_technical_step_group_is_singular_and_summary_orders_by_first_seen", () => {
    const t = trace([
      ["p", "plan"],
      ["e", "evidence_request"],
      ["a", "approval"],
    ]);
    expect(groups(build(t, "standard").model)[0]?.title).toBe("1 technical step");
    const mixed = trace([
      ["r", "reasoning_turn"],
      ["e", "evidence_request"],
      ["r2", "reasoning_turn"],
    ]);
    expect(groups(build(mixed, "standard").model)[0]?.summary).toBe("Model turn ×2, Evidence");
  });

  it("essential_profile_folds_like_standard", () => {
    expect(build(TECHNICAL, "essential").model).toEqual(build(TECHNICAL, "standard").model);
  });

  it("problem_outcomes_are_never_folded", () => {
    const t = trace([
      ["e1", "evidence_request"],
      ["e2", "evidence_request", "unavailable"],
      ["e3", "evidence_request"],
    ]);
    const { model } = build(t, "standard");
    expect(ids(model)).toEqual(["group:e1", "e2", "group:e3"]);
    expect(model.nodes[1]?.kind).toBe("step");
  });

  it("a_run_is_split_where_the_cycle_changes", () => {
    const t = trace([
      ["a", "evidence_request", "succeeded", 0],
      ["b", "reasoning_turn", "succeeded", 0],
      ["c", "evidence_request", "succeeded", 1],
      ["d", "reasoning_turn", "succeeded", 1],
    ]);
    const { model } = build(t, "standard");
    expect(groups(model).map((g) => g.members)).toEqual([
      ["a", "b"],
      ["c", "d"],
    ]);
  });

  it("edges_are_contracted_onto_group_nodes_without_duplicates_or_self_loops", () => {
    const t = trace(
      [
        ["p", "plan"],
        ["e1", "evidence_request"],
        ["e2", "evidence_request"],
        ["a", "approval"],
      ],
      [
        ["e1", "p", "depends_on"],
        ["e2", "p", "depends_on"],
        ["e2", "e1", "depends_on"],
        ["a", "e1", "depends_on"],
        ["a", "e2", "depends_on"],
        ["a", "e2", "caused_by"],
      ],
    );
    const { model } = build(t, "standard");
    expect(model.edges.map((e) => e.id).sort()).toEqual(
      ["a>group:e1:caused_by", "a>group:e1:depends_on", "group:e1>p:depends_on"].sort(),
    );
    expect(model.edges.find((e) => e.id === "a>group:e1:caused_by")).toMatchObject({
      from: "a",
      to: "group:e1",
      kind: "caused_by",
      label: "Caused by",
    });
  });

  it("contraction_never_creates_a_cycle", () => {
    for (let seed = 1; seed <= 20; seed += 1) {
      const t = makeTrace(200, seed);
      for (const profile of ["standard", "essential"] as const) {
        const { model, foldedGroups } = build(t, profile);
        expect(foldedGroups).toBe(groups(model).length);
        expect(foldedGroups).toBeGreaterThan(0);
        // Kahn: repeatedly remove nodes with no prerequisite left.
        const left = new Map(model.nodes.map((n) => [n.id, 0]));
        for (const e of model.edges) left.set(e.from, (left.get(e.from) ?? 0) + 1);
        const ready = [...left].filter(([, n]) => n === 0).map(([id]) => id);
        let removed = 0;
        while (ready.length > 0) {
          const id = ready.pop() as string;
          removed += 1;
          for (const e of model.edges.filter((x) => x.to === id)) {
            left.set(e.from, (left.get(e.from) ?? 1) - 1);
            if (left.get(e.from) === 0) ready.push(e.from);
          }
        }
        expect(removed).toBe(model.nodes.length);
      }
    }
  });

  it("a_group_never_holds_a_p0_or_p1_step_in_a_synthetic_trace", () => {
    const t = makeTrace(200, 1);
    const kinds = new Map(t.steps.map((s) => [s.id, s]));
    for (const g of groups(build(t, "standard").model)) {
      for (const m of g.members) {
        const s = kinds.get(m);
        expect(["evidence_request", "reasoning_turn"]).toContain(s?.kind);
        expect(["failed", "outcome_unknown"]).not.toContain(s?.outcome);
      }
    }
  });

  it("an_open_group_shows_its_members_as_ordinary_nodes", () => {
    const { model } = build(TECHNICAL, "standard", { expanded: new Set(["group:e1"]) });
    expect(ids(model)).toEqual(["p", "e1", "e2", "e3", "r1", "a"]);
    expect(model.edges.map((e) => e.id).sort()).toEqual(
      ["e1>p:depends_on", "e2>e1:depends_on", "r1>e3:depends_on", "a>r1:depends_on"].sort(),
    );
  });

  it("the_selected_step_opens_its_group", () => {
    const t = trace([
      ["e1", "evidence_request"],
      ["a", "approval"],
      ["e2", "evidence_request"],
      ["e3", "evidence_request"],
    ]);
    const { model, groupOf } = build(t, "standard", { selected: "e2" });
    expect(ids(model)).toEqual(["group:e1", "a", "e2", "e3"]);
    expect(groupOf.get("e2")).toBe("group:e2");
  });

  it("windowing_keeps_non_final_unknown_result_and_selected_steps", () => {
    const t = makeTrace(300, 1);
    const copy = structuredClone(t);
    const unknown = copy.steps[5];
    if (unknown) unknown.outcome = "outcome_unknown";
    const expected = copy.steps
      .map((s, i) => ({ s, i }))
      .filter(({ i }) => i >= 100 || (i + 1) % 23 === 0 || i === 5 || i === 7)
      .map(({ s }) => s.id);
    const built = build(copy, "detailed", { selected: "s7" });
    expect(ids(built.model)).toEqual(expected);
    expect(built.hiddenEarlier).toBe(300 - expected.length);
    const all = build(copy, "detailed", { showAll: true });
    expect(all.model.nodes).toHaveLength(300);
    expect(all.hiddenEarlier).toBe(0);
    expect(build(copy, "detailed", { windowLimit: 10 }).model.nodes.length).toBeLessThan(
      built.model.nodes.length,
    );
  });

  it("edges_to_windowed_out_steps_are_dropped", () => {
    const { model } = build(makeTrace(300, 1), "detailed");
    const present = new Set(ids(model));
    expect(model.edges.length).toBeGreaterThan(0);
    for (const e of model.edges) {
      expect(present.has(e.from)).toBe(true);
      expect(present.has(e.to)).toBe(true);
    }
  });

  it("unknown_kind_outcome_and_edge_kind_render_generically_and_never_throw", () => {
    const copy = structuredClone(TRACE_FIXTURES["verified-success"]) as OperationTrace;
    const first = copy.steps[0];
    if (first) {
      first.kind = "from_the_future";
      first.outcome = "odd";
    }
    const edge = copy.edges[0];
    if (edge) edge.kind = "links";
    for (const profile of ["detailed", "standard", "essential"] as const) {
      const { model } = build(copy, profile);
      const node = model.nodes.find((n) => n.id === first?.id);
      expect(node).toMatchObject({
        title: "from_the_future",
        status: { label: "odd", tone: "neutral", value: "odd" },
      });
      expect(model.edges.find((e) => e.kind === "links")?.label).toBe("links");
    }
  });

  it("retry_marks_a_step_that_has_a_retry_of_edge", () => {
    const t = TRACE_FIXTURES["failed-verification-new-cycle"] as OperationTrace;
    const retrying = new Set(t.edges.filter((e) => e.kind === "retry_of").map((e) => e.from));
    expect(retrying.size).toBe(1);
    for (const n of build(t, "detailed").model.nodes) {
      expect(n.retry).toBe(retrying.has(n.id));
    }
  });

  it("summary_null_gives_a_node_without_a_summary", () => {
    const t = TRACE_FIXTURES["verified-success"] as OperationTrace;
    const plan = build(t, "detailed").model.nodes[1];
    expect(plan?.title).toBe("Plan");
    expect(plan?.summary).toBeNull();
  });

  it("a_trace_with_no_steps_builds_an_empty_model", () => {
    const built = build(trace([]), "standard");
    expect(built.model.nodes).toEqual([]);
    expect(built.model.edges).toEqual([]);
    expect(built.hiddenEarlier).toBe(0);
  });
});
