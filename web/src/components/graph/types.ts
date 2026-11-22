import type { Ref } from "react";
import type { IconName } from "../../design/icons";

/** One node as the engine draws it. All words are resolved by the caller; the kit knows no vocabulary. */
export interface GraphNodeModel {
  /** A step id, or `group:<first member step id>` for a folded group. */
  id: string;
  /** Position in time order (a group: its first member's). */
  ordinal: number;
  kind: "step" | "group";
  /** The kind label ("Evidence"); a group: "3 technical steps". */
  title: string;
  icon: IconName;
  /** `value` is the raw outcome ("running"), "" for a group. */
  status: { label: string; tone: string; icon: IconName; value: string };
  /** Plain text; a group: the kinds counted ("Model turn ×2, Evidence"). */
  summary: string | null;
  /** A group: its step ids in order; a step: empty. */
  members: readonly string[];
  /** The step has a `retry_of` edge. */
  retry: boolean;
}

/** One edge: `from` is the dependent step, `to` its prerequisite. */
export interface GraphEdgeModel {
  id: string;
  from: string;
  to: string;
  kind: string;
  label: string;
}

/** What the graph draws: a pure function of the trace it was built from. */
export interface GraphModel {
  /** In ordinal order. */
  nodes: readonly GraphNodeModel[];
  edges: readonly GraphEdgeModel[];
  /** Changes exactly when the set of node ids or edge ids changes; a status-only change keeps it. */
  layoutKey: string;
}

/** A node centre in graph space (layout units), not client pixels. */
export interface NodePosition {
  id: string;
  x: number;
  y: number;
}

/** What the shell gives the engine. */
export interface GraphEngineProps {
  model: GraphModel;
  /** Drawn with the focus ring while `rootFocused`. */
  focusedId: string | null;
  /** A step id; drawn selected. */
  selectedId: string | null;
  rootFocused: boolean;
  /** Node ids that may animate for THIS update (at most five). Every other change is instant. */
  transitions: ReadonlySet<string>;
  /** False for large graphs and when the caller asks for none. */
  animateLayout: boolean;
  /** Click or tap on a node. */
  onPointerSelect(id: string): void;
  /** Double-click on a node. */
  onPointerOpen(id: string): void;
  /** Click on empty space. */
  onBackground(): void;
  /** After every layout, and once at mount when empty. */
  onLayout(positions: readonly NodePosition[], layoutKey: string): void;
  /** Layout or draw failed: the shell shows the fallback. */
  onFailure(message: string): void;
  ref?: Ref<GraphEngineHandle>;
}

/** What the shell can ask of the engine. */
export interface GraphEngineHandle {
  /** Fit all nodes in view, never above 100 % zoom. */
  fit(): void;
  /** Keeps the view centre; clamps to 25 %..200 %. */
  zoomBy(factor: number): void;
  /** Pan so the node is inside the view with a margin; no zoom change. */
  reveal(id: string): void;
  positions(): readonly NodePosition[];
}

/** Props of the kit's step graph. */
export interface StepGraphProps {
  model: GraphModel;
  /** A step id or null. */
  selectedId: string | null;
  /** Accessible name of the region; default "Step graph". */
  label?: string;
  /** Default true; false = every change instant. */
  animate?: boolean;
  /** Default "live"; changing it clears the remembered emphasis. */
  rememberKey?: string;
  /** Click or tap on a step node. */
  onSelect(id: string): void;
  /** Enter on a step, double-click: select it and move focus into the inspector. */
  onOpen(id: string): void;
  /** Escape with a selection, click on empty space. */
  onClear(): void;
  /** Enter or click on a group node. */
  onToggleGroup(groupId: string): void;
  /** Present when at least one group is open: the toolbar then shows "Fold technical steps". */
  onFoldAll?: () => void;
  /** Receives the focusable root, so the inspector can return focus to it. */
  openerRef?: Ref<HTMLElement>;
}
