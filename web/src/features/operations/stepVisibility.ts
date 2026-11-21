import type { TraceStep } from "../../api/operationTrace";
import { presentationFor, type Category, type ProfileId } from "../../design/visibility";

/** Outcomes that say something went wrong or is unknown: they make any step P0. */
export const PROBLEM_OUTCOMES: ReadonlySet<string> = new Set([
  "failed",
  "refused",
  "not_run",
  "unavailable",
  "outcome_unknown",
]);

const KNOWN_OUTCOMES: ReadonlySet<string> = new Set([
  ...PROBLEM_OUTCOMES,
  "pending",
  "running",
  "succeeded",
  "skipped",
  "cancelled",
]);

/** The base category of each step kind. An unknown kind is P1: the unclassifiable is shown. */
export const KIND_CATEGORY: Readonly<Record<string, Category>> = {
  approval: "P0",
  execution: "P0",
  verification: "P0",
  question: "P0",
  assessment: "P1",
  plan: "P1",
  policy_decision: "P1",
  notification: "P1",
  note: "P1",
  evidence_request: "P2",
  reasoning_turn: "P2",
};

function isProblem(outcome: string): boolean {
  return PROBLEM_OUTCOMES.has(outcome) || !KNOWN_OUTCOMES.has(outcome);
}

/** The category of a step row: its kind's, raised to P0 by a problem or unknown outcome. */
export function stepCategory(kind: string, outcome: string): Category {
  if (isProblem(outcome)) {
    return "P0";
  }
  return KIND_CATEGORY[kind] ?? "P1";
}

/** The category of a step's reason: P0 when something went wrong or is unknown, else P1. */
export function reasonCategory(outcome: string): Category {
  return isProblem(outcome) ? "P0" : "P1";
}

/**
 * The most important category among a step and all its descendants, so a collapsed technical
 * parent never hides a more important child.
 */
export function treeCategory(steps: readonly TraceStep[], id: string): Category {
  const children = new Map<string, TraceStep[]>();
  for (const step of steps) {
    if (step.parent) {
      const list = children.get(step.parent);
      if (list) {
        list.push(step);
      } else {
        children.set(step.parent, [step]);
      }
    }
  }
  const byId = new Map(steps.map((step) => [step.id, step]));
  const seen = new Set<string>();
  let best: Category = "P3";
  const stack = [id];
  while (stack.length > 0) {
    const at = stack.pop() as string;
    if (seen.has(at)) {
      continue;
    }
    seen.add(at);
    const step = byId.get(at);
    if (step) {
      const own = stepCategory(step.kind, step.outcome);
      if (own < best) {
        best = own;
      }
    }
    for (const child of children.get(at) ?? []) {
      stack.push(child.id);
    }
  }
  return best;
}

/** A list entry: a single item, or a run of consecutive items folded together. */
export type OutlineItem<T> = { item: T; type: "item" } | { items: T[]; type: "group" };

/**
 * Consecutive items whose presentation is not "shown" in `profile` become one group; the rest
 * stay single, in order. In Detailed nothing is grouped.
 */
export function groupSteps<T>(
  items: readonly T[],
  profile: ProfileId,
  categoryOf: (item: T) => Category,
): OutlineItem<T>[] {
  const out: OutlineItem<T>[] = [];
  for (const item of items) {
    if (presentationFor(profile, categoryOf(item)) === "shown") {
      out.push({ type: "item", item });
      continue;
    }
    const last = out[out.length - 1];
    if (last?.type === "group") {
      last.items.push(item);
    } else {
      out.push({ type: "group", items: [item] });
    }
  }
  return out;
}
