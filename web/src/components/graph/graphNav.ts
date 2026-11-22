/** One node as the keyboard model sees it: a centre in layout units plus what the `a` key needs. */
export interface NavNode {
  id: string;
  x: number;
  y: number;
  /** Position in time order (a group: its first member's). */
  ordinal: number;
  /** The raw outcome of a step; "" for a group. */
  outcome: string;
  kind: "step" | "group";
  /** The raw step kind; "" for a group. */
  stepKind: string;
}

/** `from` is the dependent, `to` its prerequisite. */
export interface NavEdge {
  from: string;
  to: string;
}

/** The keys the model answers; the shell maps `+`, `-`, `0`, Enter and Escape itself. */
export type NavKey =
  | "ArrowLeft"
  | "ArrowRight"
  | "ArrowUp"
  | "ArrowDown"
  | "Home"
  | "End"
  | "PageUp"
  | "PageDown"
  | "a";

/** Layout units: two centres whose x differ by at most this are in one layer. */
export const LAYER_TOLERANCE = 8;

/** The step that most needs a person's eye, never a group (6.4). */
function activeStep(nodes: readonly NavNode[]): NavNode | undefined {
  const steps = nodes.filter((node) => node.kind === "step");
  return (
    steps.find((node) => node.outcome === "running") ??
    steps.find(
      (node) =>
        node.outcome === "pending" &&
        (node.stepKind === "approval" || node.stepKind === "question"),
    ) ??
    steps.find((node) => node.outcome === "pending") ??
    steps[steps.length - 1]
  );
}

/** The node of `candidates` nearest to `from` vertically; ties go to the lowest ordinal. */
function nearest(from: NavNode, candidates: readonly NavNode[]): NavNode | undefined {
  let best: NavNode | undefined;
  for (const node of candidates) {
    if (
      !best ||
      Math.abs(node.y - from.y) < Math.abs(best.y - from.y) ||
      (Math.abs(node.y - from.y) === Math.abs(best.y - from.y) && node.ordinal < best.ordinal)
    ) {
      best = node;
    }
  }
  return best;
}

function inLayer(a: NavNode, b: NavNode): boolean {
  return Math.abs(a.x - b.x) <= LAYER_TOLERANCE;
}

/**
 * The node id to focus next, or null when the key moves nothing. `current` null (or an id that
 * is not in `nodes`) means nothing has focus yet. Total: the result is an id of `nodes` or null.
 */
export function nextNode(input: {
  nodes: readonly NavNode[];
  edges: readonly NavEdge[];
  current: string | null;
  key: NavKey;
}): string | null {
  const { edges, key } = input;
  const nodes = [...input.nodes].sort((a, b) => a.ordinal - b.ordinal);
  const first = nodes[0];
  if (!first) {
    return null;
  }
  const last = nodes[nodes.length - 1] as NavNode;
  const at = nodes.find((node) => node.id === input.current);
  if (key === "a") {
    return activeStep(nodes)?.id ?? null;
  }
  if (key === "End") {
    return last.id;
  }
  if (!at) {
    return first.id;
  }
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const index = nodes.indexOf(at);
  const linked = (side: "from" | "to"): NavNode[] =>
    edges
      .filter((edge) => edge[side] === at.id)
      .map((edge) => byId.get(side === "from" ? edge.to : edge.from))
      .filter((node): node is NavNode => node !== undefined);
  switch (key) {
    case "Home":
      return first.id;
    case "PageDown":
      return nodes[index + 1]?.id ?? null;
    case "PageUp":
      return nodes[index - 1]?.id ?? null;
    case "ArrowRight":
      return (
        nearest(
          at,
          linked("to").filter((node) => node.x - at.x > LAYER_TOLERANCE),
        )?.id ?? null
      );
    case "ArrowLeft":
      return (
        nearest(
          at,
          linked("from").filter((node) => at.x - node.x > LAYER_TOLERANCE),
        )?.id ?? null
      );
    case "ArrowDown":
      return (
        nearest(
          at,
          nodes.filter((node) => node !== at && inLayer(at, node) && node.y > at.y),
        )?.id ?? null
      );
    case "ArrowUp":
      return (
        nearest(
          at,
          nodes.filter((node) => node !== at && inLayer(at, node) && node.y < at.y),
        )?.id ?? null
      );
    default:
      return null;
  }
}
