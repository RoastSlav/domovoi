import type { Animator } from "./animate";
import { easeOut } from "./animate";
import { pathOf, translate } from "./engineParts";
import type { LayoutResult, Point } from "./layout";

/** Where each node corner and edge route is drawn right now (it changes while a tween runs). */
export interface Shown {
  nodes: Map<string, Point>;
  edges: Map<string, readonly Point[]>;
}

const mix = (a: number, b: number, k: number): number => a + (b - a) * k;

/**
 * Moves every node and edge from where it is drawn to its new place in `layout`, writing the
 * `transform` and `d` attributes on `requestAnimationFrame`. A node that is new starts at its
 * place; an edge whose number of points changed jumps. `ms` of 0 places everything at once.
 */
export function tweenLayout(
  animator: Animator,
  svg: SVGSVGElement,
  layout: LayoutResult,
  shown: Shown,
  ms: number,
): void {
  const nodeEls = new Map<string, Element>();
  svg.querySelectorAll("[data-node-id]").forEach((el) => {
    nodeEls.set(el.getAttribute("data-node-id") ?? "", el);
  });
  const edgeEls = new Map<string, Element>();
  svg.querySelectorAll("[data-edge-id]").forEach((el) => {
    edgeEls.set(el.getAttribute("data-edge-id") ?? "", el);
  });
  const nodes = [...layout.corners].map(([id, to]) => ({
    id,
    to,
    from: shown.nodes.get(id) ?? to,
  }));
  const edges = [...layout.routes].map(([id, to]) => {
    const from = shown.edges.get(id);
    return { id, to, from: from && from.length === to.length ? from : to };
  });
  animator.run(ms, (k) => {
    const e = easeOut(k);
    shown.nodes.clear();
    shown.edges.clear();
    for (const { id, from, to } of nodes) {
      const at: Point = [mix(from[0], to[0], e), mix(from[1], to[1], e)];
      shown.nodes.set(id, at);
      nodeEls.get(id)?.setAttribute("transform", translate(at[0], at[1]));
    }
    for (const { id, from, to } of edges) {
      const route = to.map((p, i): Point => {
        const f = from[i] ?? p;
        return [mix(f[0], p[0], e), mix(f[1], p[1], e)];
      });
      shown.edges.set(id, route);
      edgeEls.get(id)?.setAttribute("d", pathOf(route));
    }
  });
}
