import type { OperationTrace, TraceStep } from "../../../api/operationTrace";
import type { GraphEdgeModel, GraphModel, GraphNodeModel } from "../../../components";
import { edgeKindMeta, stepKindMeta, stepOutcomeMeta } from "../../../design/tone";
import type { ProfileId } from "../../../design/visibility";
import { windowSteps } from "../steps";
import { stepCategory } from "../stepVisibility";
import { foldRuns } from "./foldRuns";

/** What `buildGraphModel` needs. */
export interface BuildInput {
  trace: OperationTrace;
  profile: ProfileId;
  /** Group ids the person opened. */
  expanded: ReadonlySet<string>;
  /** A step id: the group containing it is open. */
  selected: string | null;
  /** "Show N earlier steps" was pressed. */
  showAll: boolean;
  /** Default 200, the outline's rule. */
  windowLimit?: number;
}

/** The model plus what the feature needs to wire it. */
export interface BuiltGraph {
  model: GraphModel;
  /** Steps left out by the window. */
  hiddenEarlier: number;
  /** Step id to the id of the group it belongs to, for every member of every group, open or folded. */
  groupOf: ReadonlyMap<string, string>;
  /** The number of group nodes in the model. */
  foldedGroups: number;
}

/** The id of the group node that starts at `firstStepId`. */
export function groupId(firstStepId: string): string {
  return `group:${firstStepId}`;
}

function stepNode(trace: OperationTrace, step: TraceStep, ordinal: number): GraphNodeModel {
  const kind = stepKindMeta(step.kind);
  const outcome = stepOutcomeMeta(step.outcome);
  return {
    id: step.id,
    ordinal,
    kind: "step",
    title: kind.label,
    icon: kind.icon,
    status: { label: outcome.label, tone: outcome.tone, icon: outcome.icon, value: step.outcome },
    summary: step.summary ?? null,
    members: [],
    retry: trace.edges.some((edge) => edge.from === step.id && edge.kind === "retry_of"),
    stepKind: step.kind,
  };
}

function groupNode(members: readonly TraceStep[], ordinal: number): GraphNodeModel {
  const counts = new Map<string, number>();
  for (const member of members) {
    const label = stepKindMeta(member.kind).label;
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  const summary = [...counts].map(([label, n]) => (n > 1 ? `${label} ×${n}` : label)).join(", ");
  const first = members[0] as TraceStep;
  return {
    id: groupId(first.id),
    ordinal,
    kind: "group",
    title: `${members.length} technical ${members.length === 1 ? "step" : "steps"}`,
    icon: "more",
    status: { label: "Folded", tone: "neutral", icon: "dot", value: "" },
    summary,
    members: members.map((member) => member.id),
    retry: false,
  };
}

/** Whether the directed graph has a cycle (Kahn's algorithm). */
function hasCycle(nodes: readonly GraphNodeModel[], edges: readonly GraphEdgeModel[]): boolean {
  const indegree = new Map(nodes.map((node) => [node.id, 0]));
  for (const edge of edges) {
    indegree.set(edge.to, (indegree.get(edge.to) ?? 0) + 1);
  }
  const queue = [...indegree].filter(([, n]) => n === 0).map(([id]) => id);
  let seen = 0;
  while (queue.length > 0) {
    const id = queue.pop() as string;
    seen += 1;
    for (const edge of edges) {
      if (edge.from === id) {
        const left = (indegree.get(edge.to) ?? 0) - 1;
        indegree.set(edge.to, left);
        if (left === 0) {
          queue.push(edge.to);
        }
      }
    }
  }
  return seen !== nodes.length;
}

/**
 * Turns a trace into the graph the kit draws: windowed to the newest steps, technical runs
 * folded per the profile (never a P0 step, never the selected one), edges contracted onto the
 * node that stands for each end. Pure; an unknown kind, outcome or edge kind renders generically.
 */
export function buildGraphModel(input: BuildInput): BuiltGraph {
  const { trace, profile, expanded, selected } = input;
  const ordinalOf = new Map(trace.steps.map((step, index) => [step.id, index]));
  const visible = windowSteps(trace.steps, {
    limit: input.windowLimit ?? 200,
    showAll: input.showAll,
    keep: selected === null ? [] : [selected],
  });
  const items = foldRuns(
    visible,
    profile,
    (step) => stepCategory(step.kind, step.outcome),
    (step) => step.cycle,
  );
  const nodes: GraphNodeModel[] = [];
  const rep = new Map<string, string>();
  const groupOf = new Map<string, string>();
  for (const item of items) {
    if (item.type === "item") {
      rep.set(item.item.id, item.item.id);
      nodes.push(stepNode(trace, item.item, ordinalOf.get(item.item.id) ?? 0));
      continue;
    }
    const first = item.items[0] as TraceStep;
    const id = groupId(first.id);
    for (const member of item.items) {
      groupOf.set(member.id, id);
    }
    const open =
      expanded.has(id) ||
      (selected !== null && groupOf.has(selected) && groupOf.get(selected) === id);
    if (open) {
      for (const member of item.items) {
        rep.set(member.id, member.id);
        nodes.push(stepNode(trace, member, ordinalOf.get(member.id) ?? 0));
      }
    } else {
      for (const member of item.items) {
        rep.set(member.id, id);
      }
      nodes.push(groupNode(item.items, ordinalOf.get(first.id) ?? 0));
    }
  }
  const edges: GraphEdgeModel[] = [];
  const seen = new Set<string>();
  for (const edge of trace.edges) {
    const a = rep.get(edge.from);
    const b = rep.get(edge.to);
    if (a === undefined || b === undefined || a === b) {
      continue;
    }
    const key = `${a}>${b}:${edge.kind}`;
    if (!seen.has(key)) {
      seen.add(key);
      edges.push({
        id: key,
        from: a,
        to: b,
        kind: edge.kind,
        label: edgeKindMeta(edge.kind).label,
      });
    }
  }
  if (profile !== "detailed" && hasCycle(nodes, edges)) {
    return buildGraphModel({ ...input, profile: "detailed" });
  }
  const layoutKey = `${nodes
    .map((n) => n.id)
    .sort()
    .join(",")}|${edges
    .map((e) => e.id)
    .sort()
    .join(",")}`;
  return {
    model: { nodes, edges, layoutKey },
    hiddenEarlier: trace.steps.length - visible.length,
    groupOf,
    foldedGroups: nodes.filter((node) => node.kind === "group").length,
  };
}
