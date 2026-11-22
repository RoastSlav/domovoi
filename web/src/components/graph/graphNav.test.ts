import { describe, expect, it } from "vitest";
import type { OperationTrace } from "../../api/operationTrace";
import { TRACE_FIXTURES } from "../../test/operationFixtures";
import { nextNode, type NavEdge, type NavKey, type NavNode } from "./graphNav";

function node(
  id: string,
  ordinal: number,
  x: number,
  y: number,
  extra: Partial<NavNode> = {},
): NavNode {
  return { id, ordinal, x, y, outcome: "succeeded", kind: "step", stepKind: "note", ...extra };
}

// A(0) is the only prerequisite of B and C; D needs both. B and C sit in one layer.
//   A y50 -> B y0, C y100 -> D y0
const NODES = [
  node("A", 0, 0, 50),
  node("B", 1, 300, 0),
  node("C", 2, 300, 100),
  node("D", 3, 600, 0),
];
const EDGES: NavEdge[] = [
  { from: "B", to: "A" },
  { from: "C", to: "A" },
  { from: "D", to: "B" },
  { from: "D", to: "C" },
];
const press = (current: string | null, key: NavKey, nodes = NODES, edges = EDGES) =>
  nextNode({ nodes, edges, current, key });

describe("graph keyboard model", () => {
  it("arrow_right_selects_the_nearest_dependent_in_a_layer_to_the_right", () => {
    // B and C are both 50 away from A: the lowest ordinal wins.
    expect(press("A", "ArrowRight")).toBe("B");
    expect(press("B", "ArrowRight")).toBe("D");
    expect(press("D", "ArrowRight")).toBeNull();
    // Nearest vertically beats ordinal.
    const moved = [node("A", 0, 0, 90), ...NODES.slice(1)];
    expect(press("A", "ArrowRight", moved)).toBe("C");
  });

  it("arrow_left_selects_a_prerequisite_in_a_layer_to_the_left", () => {
    expect(press("D", "ArrowLeft")).toBe("B");
    expect(press("C", "ArrowLeft")).toBe("A");
    expect(press("A", "ArrowLeft")).toBeNull();
  });

  it("arrow_up_and_down_stay_in_the_layer", () => {
    expect(press("B", "ArrowDown")).toBe("C");
    expect(press("C", "ArrowUp")).toBe("B");
    expect(press("C", "ArrowDown")).toBeNull();
    expect(press("B", "ArrowUp")).toBeNull();
    expect(press("A", "ArrowDown")).toBeNull();
    // A centre 8 units off still belongs to the layer, 9 does not (B to C).
    const near = [node("A", 0, 0, 0), node("B", 1, 8, 50), node("C", 2, 17, 100)];
    expect(press("A", "ArrowDown", near, [])).toBe("B");
    expect(press("B", "ArrowDown", near, [])).toBeNull();
  });

  it("page_down_and_page_up_walk_every_node_in_time_order", () => {
    const shuffled = [NODES[2], NODES[0], NODES[3], NODES[1]] as NavNode[];
    const walked: string[] = ["A"];
    for (let at: string | null = "A"; at !== null;) {
      at = press(at, "PageDown", shuffled, []);
      if (at !== null) walked.push(at);
    }
    expect(walked).toEqual(["A", "B", "C", "D"]);
    expect(press("D", "PageDown")).toBeNull();
    expect(press("C", "PageUp")).toBe("B");
    expect(press("A", "PageUp")).toBeNull();
  });

  it("home_and_end_select_the_first_and_last_ordinal", () => {
    const group = node("group:x", 4, 900, 0, { kind: "group", stepKind: "" });
    const withGroup = [group, ...NODES];
    expect(press("C", "Home", withGroup)).toBe("A");
    expect(press("C", "End", withGroup)).toBe("group:x");
    expect(press(null, "End")).toBe("D");
  });

  it("a_selects_running_then_a_pending_approval_or_question_then_pending_then_the_last", () => {
    const base = (outcomes: Partial<NavNode>[]) =>
      outcomes.map((o, i) => node(`n${i}`, i, i * 300, 0, o));
    const key = (nodes: NavNode[]) => nextNode({ nodes, edges: [], current: null, key: "a" });
    // running wins over everything, lowest ordinal first
    expect(
      key(
        base([
          { outcome: "pending", stepKind: "approval" },
          { outcome: "running" },
          { outcome: "running" },
        ]),
      ),
    ).toBe("n1");
    // no running: a pending approval or question beats an earlier plain pending step
    expect(
      key(base([{ outcome: "pending" }, { outcome: "pending", stepKind: "question" }, {}])),
    ).toBe("n1");
    // plain pending
    expect(key(base([{}, { outcome: "pending" }, {}]))).toBe("n1");
    // nothing open: the last step; a group is never chosen, even when it is last or running
    const group = node("g", 5, 0, 0, { kind: "group", stepKind: "", outcome: "running" });
    expect(key([...base([{}, {}]), group])).toBe("n1");
    expect(key([group])).toBeNull();
  });

  it("an_arrow_with_nothing_focused_focuses_the_first_node", () => {
    const keys: NavKey[] = [
      "ArrowLeft",
      "ArrowRight",
      "ArrowUp",
      "ArrowDown",
      "PageDown",
      "PageUp",
      "Home",
    ];
    for (const key of keys) {
      expect(press(null, key)).toBe("A");
    }
    expect(press("not-a-node", "ArrowDown")).toBe("A");
  });

  it("a_key_that_moves_nothing_returns_null", () => {
    expect(press("D", "ArrowRight")).toBeNull();
    expect(press(null, "Home", [], [])).toBeNull();
  });

  it("nav_never_throws_on_unknown_outcomes_and_kinds", () => {
    const odd = NODES.map((n) => ({ ...n, outcome: "future", stepKind: "future_kind" }));
    const keys: NavKey[] = [
      "ArrowLeft",
      "ArrowRight",
      "ArrowUp",
      "ArrowDown",
      "Home",
      "End",
      "PageUp",
      "PageDown",
      "a",
    ];
    const ids = new Set(odd.map((n) => n.id));
    for (const key of keys) {
      for (const current of [null, ...ids]) {
        const result = nextNode({ nodes: odd, edges: EDGES, current, key });
        expect(result === null || ids.has(result)).toBe(true);
      }
    }
  });

  it("an_edge_to_a_missing_node_is_ignored", () => {
    expect(press("A", "ArrowRight", NODES, [{ from: "ghost", to: "A" }])).toBeNull();
  });
});

interface Laid {
  nodes: NavNode[];
  edges: NavEdge[];
}

/** Rank by longest path, rows in ordinal order within a rank. */
function lay(trace: OperationTrace): Laid {
  const rank = new Map<string, number>();
  for (const step of trace.steps) {
    const prerequisites = trace.edges
      .filter((e) => e.from === step.id)
      .map((e) => rank.get(e.to) ?? 0);
    rank.set(step.id, prerequisites.length === 0 ? 0 : Math.max(...prerequisites) + 1);
  }
  const rows = new Map<number, number>();
  const nodes = trace.steps.map((step, ordinal) => {
    const r = rank.get(step.id) as number;
    const row = rows.get(r) ?? 0;
    rows.set(r, row + 1);
    return node(step.id, ordinal, r * 300, row * 100, {
      outcome: step.outcome,
      stepKind: step.kind,
    });
  });
  return { nodes, edges: trace.edges.map((e) => ({ from: e.from, to: e.to })) };
}

const FIXTURES: [string, OperationTrace][] = [
  ["waiting-approval", TRACE_FIXTURES["waiting-approval"]],
  ["verified-success", TRACE_FIXTURES["verified-success"]],
  ["failed-verification-new-cycle", TRACE_FIXTURES["failed-verification-new-cycle"]],
  ["outcome-unknown-paused", TRACE_FIXTURES["outcome-unknown-paused"]],
  ["question-and-budget-pause", TRACE_FIXTURES["question-and-budget-pause"]],
];

describe.each(FIXTURES)("keyboard reach in %s", (_name, trace) => {
  it("arrow_keys_reach_every_connected_node_of_each_fixture", () => {
    const { nodes, edges } = lay(trace);
    const first = nodes[0] as NavNode;
    const arrows: NavKey[] = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];
    const reached = new Set([first.id]);
    const queue = [first.id];
    while (queue.length > 0) {
      const at = queue.shift() as string;
      for (const key of arrows) {
        const to = nextNode({ nodes, edges, current: at, key });
        if (to !== null && !reached.has(to)) {
          reached.add(to);
          queue.push(to);
        }
      }
    }
    // Expected, by hand: the undirected edge component of the first node.
    const component = new Set([first.id]);
    for (let grew = true; grew;) {
      grew = false;
      for (const e of edges) {
        if (component.has(e.from) !== component.has(e.to)) {
          component.add(e.from).add(e.to);
          grew = true;
        }
      }
    }
    for (const id of component) {
      expect(reached.has(id)).toBe(true);
    }
    // Whatever arrows cannot reach has no edge at all.
    const unreachable = nodes.filter((n) => !reached.has(n.id)).map((n) => n.id);
    const edgeFree = nodes
      .filter((n) => !edges.some((e) => e.from === n.id || e.to === n.id))
      .map((n) => n.id);
    for (const id of unreachable) {
      expect(edgeFree).toContain(id);
    }
    // PageDown reaches all.
    const paged = [first.id];
    for (let at = nextNode({ nodes, edges, current: first.id, key: "PageDown" }); at !== null;) {
      paged.push(at);
      at = nextNode({ nodes, edges, current: at, key: "PageDown" });
    }
    expect(paged).toEqual(trace.steps.map((s) => s.id));
  });
});
