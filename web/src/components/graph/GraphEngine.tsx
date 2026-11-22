import { useId, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";
import { motionMs } from "../../design/motion";
import { createAnimator } from "./animate";
import { EdgeShape, NodeShape } from "./engineParts";
import { layoutModel, NODE_H, NODE_W, type LayoutResult, type Point } from "./layout";
import { tweenLayout } from "./tweenLayout";
import { useViewport } from "./useViewport";
import styles from "./graph.module.css";
import type { GraphEngineProps, GraphModel, NodePosition } from "./types";

interface Computed {
  key: string;
  layout: LayoutResult | null;
  error: string | null;
}

/** Lays the model out; a throw becomes `error` so the engine never throws into React. */
function compute(model: GraphModel): Computed {
  try {
    return { key: model.layoutKey, layout: layoutModel(model), error: null };
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : "layout failed";
    return { key: model.layoutKey, layout: null, error: message };
  }
}

function centres(layout: LayoutResult | null): NodePosition[] {
  return [...(layout?.corners ?? [])].map(([id, [x, y]]) => ({
    id,
    x: x + NODE_W / 2,
    y: y + NODE_H / 2,
  }));
}

/**
 * The SVG engine: dagre lays the model out left to right, React draws plain SVG elements
 * (text is text), and every animation is a `requestAnimationFrame` tween of an SVG attribute
 * whose duration is read from `motionMs` when it starts. The engine writes no `style` attribute.
 */
export default function GraphEngine(props: GraphEngineProps) {
  const { model, focusedId, selectedId, rootFocused, transitions, animateLayout, ref } = props;
  const svgRef = useRef<SVGSVGElement>(null);
  const marker = `graph-arrow-${useId().replaceAll(":", "")}`;
  const [cache, setCache] = useState(() => compute(model));
  let current = cache;
  if (cache.key !== model.layoutKey) {
    current = compute(model);
    setCache(current);
  }
  const { layout } = current;
  const latest = useRef(props);
  useLayoutEffect(() => {
    latest.current = props;
  });
  const layoutRef = useRef<LayoutResult | null>(null);
  const viewport = useViewport(svgRef, animateLayout, () => latest.current.onBackground());
  const nodeAnimator = useMemo(() => createAnimator(), []);
  const entryAnimator = useMemo(() => createAnimator(), []);
  const edgeAnimator = useMemo(() => createAnimator(), []);
  const shown = useRef({
    nodes: new Map<string, Point>(),
    edges: new Map<string, readonly Point[]>(),
  });
  const first = useRef(true);

  useLayoutEffect(() => {
    layoutRef.current = layout;
    const { onLayout, onFailure } = latest.current;
    if (current.error !== null || !layout || !svgRef.current) {
      onFailure(current.error ?? "The graph has no layout.");
      return;
    }
    try {
      const ms = latest.current.animateLayout && !first.current ? motionMs("layout") : 0;
      tweenLayout(nodeAnimator, svgRef.current, layout, shown.current, ms);
      viewport.setGraph(layout, !first.current);
      first.current = false;
      onLayout(centres(layout), current.key);
    } catch (cause) {
      onFailure(cause instanceof Error ? cause.message : "The graph could not be drawn.");
    }
  }, [current, layout, viewport, nodeAnimator]);

  const previous = useRef<ReadonlySet<string> | null>(null);
  useLayoutEffect(() => {
    const svg = svgRef.current;
    const before = previous.current;
    previous.current = new Set(model.nodes.map((node) => node.id));
    if (!svg || !before || transitions.size === 0) {
      return;
    }
    const nodes = [...svg.querySelectorAll("[data-node-id]")].filter((el) => {
      const id = el.getAttribute("data-node-id") ?? "";
      return transitions.has(id) && !before.has(id);
    });
    const edges = [...svg.querySelectorAll("[data-edge-id]")].filter((el) => {
      const edge = model.edges.find((e) => e.id === el.getAttribute("data-edge-id"));
      return edge !== undefined && transitions.has(edge.from) && transitions.has(edge.to);
    });
    const fade = (els: Element[]) => (k: number) =>
      els.forEach((el) =>
        k >= 1
          ? el.removeAttribute("opacity")
          : el.setAttribute("opacity", String(0.15 + 0.85 * k)),
      );
    entryAnimator.run(nodes.length > 0 ? motionMs("graph") : 0, fade(nodes));
    edgeAnimator.run(edges.length > 0 ? motionMs("graph-edge") : 0, fade(edges));
  }, [model, transitions, entryAnimator, edgeAnimator]);

  useLayoutEffect(
    () => () => {
      nodeAnimator.cancel();
      entryAnimator.cancel();
      edgeAnimator.cancel();
    },
    [nodeAnimator, entryAnimator, edgeAnimator],
  );

  useImperativeHandle(
    ref,
    () => ({
      fit: viewport.fit,
      zoomBy: viewport.zoomBy,
      reveal(id) {
        const corner = layoutRef.current?.corners.get(id);
        if (corner) {
          viewport.reveal({ x: corner[0], y: corner[1], w: NODE_W, h: NODE_H });
        }
      },
      positions: () => centres(layoutRef.current),
    }),
    [viewport],
  );

  const lit = new Set<string>();
  if (selectedId) {
    lit.add(selectedId);
  }
  if (rootFocused && focusedId) {
    lit.add(focusedId);
  }
  const settling = transitions.size > 0 && motionMs("graph") > 0;
  return (
    <svg ref={svgRef} className={styles.svg} aria-hidden="true" focusable="false">
      <defs>
        <marker
          id={marker}
          viewBox="0 0 10 10"
          refX="9"
          refY="5"
          markerWidth="8"
          markerHeight="8"
          orient="auto"
        >
          <path className={styles.arrow} d="M0 0 L10 5 L0 10 z" />
        </marker>
      </defs>
      <g>
        {model.edges.map((edge) => {
          const route = layout?.routes.get(edge.id);
          return route ? (
            <EdgeShape
              key={edge.id}
              edge={edge}
              route={route}
              marker={marker}
              labelled={lit.has(edge.from) || lit.has(edge.to)}
            />
          ) : null;
        })}
      </g>
      <g>
        {model.nodes.map((node) => {
          const corner = layout?.corners.get(node.id);
          return corner ? (
            <NodeShape
              key={node.id}
              node={node}
              corner={corner}
              selected={node.id === selectedId}
              focused={rootFocused && node.id === focusedId}
              settle={settling && transitions.has(node.id)}
              onSelect={props.onPointerSelect}
              onOpen={props.onPointerOpen}
            />
          ) : null;
        })}
      </g>
    </svg>
  );
}
