import dagre from "@dagrejs/dagre";
import type { GraphModel } from "./types";

/** Node size in layout units; layout units are CSS pixels at 100 % zoom. */
export const NODE_W = 224;
export const NODE_H = 72;
/** Gap between ranks (mirrors `--space-8`) and between nodes of a rank (mirrors `--space-5`). */
export const RANK_SEP = 64;
export const NODE_SEP = 24;

/** A point in layout units. */
export type Point = readonly [number, number];

/** Where the layout put everything. Node positions are top-left corners. */
export interface LayoutResult {
  /** Top-left corner of every node. */
  corners: ReadonlyMap<string, Point>;
  /** The polyline of every edge, from the prerequisite to the dependent. */
  routes: ReadonlyMap<string, readonly Point[]>;
  width: number;
  height: number;
}

/**
 * Lays the model out left to right: prerequisites left, dependents right. Deterministic:
 * nodes enter the layout in ordinal order and edges in model order. Edges that name a node
 * outside the model are skipped.
 */
export function layoutModel(model: GraphModel): LayoutResult {
  const g = new dagre.graphlib.Graph({ multigraph: true });
  g.setGraph({ rankdir: "LR", ranksep: RANK_SEP, nodesep: NODE_SEP, marginx: 0, marginy: 0 });
  g.setDefaultEdgeLabel(() => ({}));
  const ids = new Set<string>();
  for (const node of [...model.nodes].sort((a, b) => a.ordinal - b.ordinal)) {
    g.setNode(node.id, { width: NODE_W, height: NODE_H });
    ids.add(node.id);
  }
  const kept = model.edges.filter((e) => ids.has(e.from) && ids.has(e.to) && e.from !== e.to);
  // Dagre ranks along the edge direction: draw prerequisite -> dependent so time flows right.
  for (const edge of kept) {
    g.setEdge(edge.to, edge.from, {}, edge.id);
  }
  dagre.layout(g);
  const corners = new Map<string, Point>();
  let width = 0;
  let height = 0;
  for (const id of ids) {
    const p = g.node(id);
    corners.set(id, [p.x - NODE_W / 2, p.y - NODE_H / 2]);
    width = Math.max(width, p.x + NODE_W / 2);
    height = Math.max(height, p.y + NODE_H / 2);
  }
  const routes = new Map<string, readonly Point[]>();
  for (const edge of kept) {
    const label = g.edge(edge.to, edge.from, edge.id);
    const points = ((label?.points ?? []) as { x: number; y: number }[]).map((p): Point => [
      p.x,
      p.y,
    ]);
    routes.set(edge.id, points);
    for (const [x, y] of points) {
      width = Math.max(width, x);
      height = Math.max(height, y);
    }
  }
  return { corners, routes, width, height };
}
