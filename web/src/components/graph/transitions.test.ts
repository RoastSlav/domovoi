import { describe, expect, it } from "vitest";
import { ANIMATE_LAYOUT_MAX_NODES, MAX_ANIMATED_NODES, planTransitions } from "./transitions";
import type { GraphModel, GraphNodeModel } from "./types";

function node(id: string, ordinal: number, value: string): GraphNodeModel {
  return {
    id,
    ordinal,
    kind: "step",
    title: "Note",
    icon: "note",
    status: { label: value, tone: "neutral", icon: "dot", value },
    summary: null,
    members: [],
    retry: false,
  };
}
const model = (nodes: GraphNodeModel[]): GraphModel => ({ nodes, edges: [], layoutKey: "k" });
const none = new Set<string>();

describe("burst batching", () => {
  it("planTransitions_animates_nothing_on_the_first_render", () => {
    const plan = planTransitions(
      null,
      model([node("a", 0, "running"), node("b", 1, "pending")]),
      none,
    );
    expect(plan.animate.size).toBe(0);
    expect([...plan.remembered].sort()).toEqual(["a:running", "b:pending"]);
  });

  it("planTransitions_animates_at_most_five_newest_changed_first", () => {
    const before = Array.from({ length: 8 }, (_, i) => node(`c${i}`, i, "pending"));
    const after = [
      ...before.map((n) => node(n.id, n.ordinal, "succeeded")),
      ...Array.from({ length: 4 }, (_, i) => node(`n${i}`, 20 + i, "pending")),
    ];
    const plan = planTransitions(model(before), model(after), none);
    expect(MAX_ANIMATED_NODES).toBe(5);
    // New nodes have higher ordinals, yet the five newest CHANGED nodes win.
    expect([...plan.animate].sort()).toEqual(["c3", "c4", "c5", "c6", "c7"]);
    for (const n of after) {
      expect(plan.remembered.has(`${n.id}:${n.status.value}`)).toBe(true);
    }
  });

  it("planTransitions_fills_the_rest_with_new_nodes_newest_first", () => {
    const before = [node("a", 0, "pending"), node("b", 1, "pending")];
    const after = [
      node("a", 0, "running"),
      node("b", 1, "pending"),
      ...[2, 3, 4, 5, 6].map((i) => node(`n${i}`, i, "pending")),
    ];
    const plan = planTransitions(model(before), model(after), none);
    expect([...plan.animate].sort()).toEqual(["a", "n3", "n4", "n5", "n6"]);
  });

  it("planTransitions_skips_a_state_already_remembered", () => {
    const before = model([node("a", 0, "pending")]);
    const after = model([node("a", 0, "running")]);
    const first = planTransitions(before, after, none);
    expect([...first.animate]).toEqual(["a"]);
    const second = planTransitions(before, after, first.remembered);
    expect(second.animate.size).toBe(0);
  });

  it("planTransitions_animates_nothing_when_the_same_model_arrives_again", () => {
    const m = model([node("a", 0, "running")]);
    const plan = planTransitions(m, m, none);
    expect(plan.animate.size).toBe(0);
  });

  it("layout_animation_limit_is_150_nodes", () => {
    expect(ANIMATE_LAYOUT_MAX_NODES).toBe(150);
  });
});
