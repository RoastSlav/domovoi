import type { GraphNodeModel } from "./types";

/** The accessible name of the graph region when the caller gives none. */
export const DEFAULT_GRAPH_LABEL = "Step graph";

/** The region's accessible name: the caller's label, else the default. */
export function graphLabel(label?: string): string {
  return label && label.trim() !== "" ? label : DEFAULT_GRAPH_LABEL;
}

/** The help text `aria-describedby` points to. */
export const GRAPH_INSTRUCTIONS =
  "Arrow keys move between connected steps. Page Down and Page Up move through steps in time order. Enter opens the step. Escape clears the selection. Plus, minus and zero zoom and fit.";

/** How many steps a node depends on, is needed by, and whether it is the selection. */
export interface LiveLineContext {
  dependsOn: number;
  neededBy: number;
  selected: boolean;
}

function steps(count: number): string {
  return `${count} ${count === 1 ? "step" : "steps"}`;
}

/**
 * The sentence the live region speaks when focus or selection moves to `node`. Never built on
 * data refresh, and the caller says nothing for an empty model or an engine that is not ready.
 */
export function liveLine(node: GraphNodeModel, context: LiveLineContext): string {
  const summary = node.summary === null ? "" : `: ${node.summary}`;
  if (node.kind === "group") {
    return `${node.title} folded${summary}. Press Enter to show them.`;
  }
  const parts = [`${node.title}, ${node.status.label}${summary}.`];
  if (context.dependsOn > 0) {
    parts.push(`Depends on ${steps(context.dependsOn)}.`);
  }
  if (context.neededBy > 0) {
    parts.push(`Needed by ${steps(context.neededBy)}.`);
  }
  if (context.selected) {
    parts.push("Selected.");
  }
  return parts.join(" ");
}
