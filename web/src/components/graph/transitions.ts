import type { GraphModel } from "./types";

/** At most this many nodes animate for one update; the rest appear in their final state. */
export const MAX_ANIMATED_NODES = 5;
/** Above this many nodes, layout changes are instant. */
export const ANIMATE_LAYOUT_MAX_NODES = 150;

/** Which nodes animate now, and the node states already shown once. */
export interface TransitionPlan {
  /** Node ids that may animate for this update. */
  animate: ReadonlySet<string>;
  /** `${id}:${status.value}` keys of every state shown so far. */
  remembered: ReadonlySet<string>;
}

/**
 * Decides which nodes animate in a burst. A candidate is a new node or one whose status changed;
 * emphasis is shown once per node and state; changed nodes go before new ones, newest first,
 * and only the first `MAX_ANIMATED_NODES` animate. `prev` null (first render, reload,
 * reconnect) animates nothing and remembers every current node state.
 */
export function planTransitions(
  prev: GraphModel | null,
  next: GraphModel,
  remembered: ReadonlySet<string>,
): TransitionPlan {
  const keyOf = (node: { id: string; status: { value: string } }): string =>
    `${node.id}:${node.status.value}`;
  const all = new Set(remembered);
  for (const node of next.nodes) {
    all.add(keyOf(node));
  }
  if (!prev) {
    return { animate: new Set(), remembered: all };
  }
  const before = new Map(prev.nodes.map((node) => [node.id, node]));
  const changed = [];
  const added = [];
  for (const node of next.nodes) {
    const old = before.get(node.id);
    if (remembered.has(keyOf(node))) {
      continue;
    }
    if (!old) {
      added.push(node);
    } else if (old.status.value !== node.status.value) {
      changed.push(node);
    }
  }
  const newestFirst = (a: { ordinal: number }, b: { ordinal: number }): number =>
    b.ordinal - a.ordinal;
  const ordered = [...changed.sort(newestFirst), ...added.sort(newestFirst)];
  const animate = new Set(ordered.slice(0, MAX_ANIMATED_NODES).map((node) => node.id));
  return { animate, remembered: all };
}
