import type { KeyboardEvent } from "react";
import { nextNode, type NavKey, type NavNode } from "./graphNav";
import { ZOOM_IN, ZOOM_OUT } from "./GraphToolbar";
import type { GraphEngineHandle, GraphModel, GraphNodeModel } from "./types";

const NAV_KEYS: ReadonlySet<string> = new Set([
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "a",
]);
const ARROWS: ReadonlySet<string> = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"]);

/** What the keyboard handler needs from the shell. */
export interface KeyContext {
  model: GraphModel;
  /** Node centres from the last layout, by id; empty until the first layout. */
  centres: ReadonlyMap<string, { x: number; y: number }>;
  focusedId: string | null;
  selectedId: string | null;
  handle: { current: GraphEngineHandle | null };
  focus(id: string): void;
  open(node: GraphNodeModel): void;
  clear(): void;
}

/** The keyboard model's view of the model: ordinal order, with centres where the layout gave them. */
export function navNodes(model: GraphModel, centres: KeyContext["centres"]): NavNode[] {
  return model.nodes.map((node) => ({
    id: node.id,
    x: centres.get(node.id)?.x ?? 0,
    y: centres.get(node.id)?.y ?? 0,
    ordinal: node.ordinal,
    outcome: node.status.value,
    kind: node.kind,
    stepKind: node.stepKind ?? "",
  }));
}

/**
 * Handles one key press on the graph root (6.4). Keys act only when the root itself is the
 * target and no modifier but Shift is held; every key it handles gets `preventDefault`, so the
 * page never scrolls under the graph. Escape is consumed only with a selection.
 */
export function handleGraphKey(event: KeyboardEvent<HTMLElement>, ctx: KeyContext): void {
  if (
    event.target !== event.currentTarget ||
    event.defaultPrevented ||
    event.ctrlKey ||
    event.metaKey ||
    event.altKey
  ) {
    return;
  }
  const { key } = event;
  if (event.shiftKey && key !== "+") {
    return;
  }
  const zoom = (factor: number): void => {
    event.preventDefault();
    ctx.handle.current?.zoomBy(factor);
  };
  if (key === "+" || key === "=") {
    return zoom(ZOOM_IN);
  }
  if (key === "-") {
    return zoom(ZOOM_OUT);
  }
  if (key === "0") {
    event.preventDefault();
    ctx.handle.current?.fit();
    return;
  }
  if (key === "Escape") {
    if (ctx.selectedId !== null) {
      event.preventDefault();
      ctx.clear();
    }
    return;
  }
  const focused = ctx.model.nodes.find((node) => node.id === ctx.focusedId);
  if (key === "Enter") {
    if (focused) {
      event.preventDefault();
      ctx.open(focused);
    }
    return;
  }
  if (!NAV_KEYS.has(key)) {
    return;
  }
  event.preventDefault();
  if (ARROWS.has(key) && ctx.centres.size === 0) {
    return;
  }
  const next = nextNode({
    nodes: navNodes(ctx.model, ctx.centres),
    edges: ctx.model.edges,
    current: focused ? focused.id : null,
    key: key as NavKey,
  });
  if (next !== null) {
    ctx.focus(next);
  }
}

/** The node the first Tab into the graph lands on: the selection, the last focus, the `a` target, the first node. */
export function entryNode(
  ctx: Omit<KeyContext, "handle" | "focus" | "open" | "clear">,
): string | null {
  const has = (id: string | null): id is string =>
    id !== null && ctx.model.nodes.some((node) => node.id === id);
  if (has(ctx.selectedId)) {
    return ctx.selectedId;
  }
  if (has(ctx.focusedId)) {
    return ctx.focusedId;
  }
  const nodes = navNodes(ctx.model, ctx.centres);
  const edges = ctx.model.edges;
  return (
    nextNode({ nodes, edges, current: null, key: "a" }) ??
    nextNode({ nodes, edges, current: null, key: "Home" })
  );
}
