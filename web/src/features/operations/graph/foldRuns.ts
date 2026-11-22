import type { Category, ProfileId } from "../../../design/visibility";
import { groupSteps, type OutlineItem } from "../stepVisibility";

/**
 * Folds runs of consecutive items the profile does not show, over the whole ordinal list, like
 * the timeline outline does, and then splits every group where the cycle changes. Runs stay
 * contiguous in ordinal order, which keeps the contracted graph acyclic.
 */
export function foldRuns<T>(
  items: readonly T[],
  profile: ProfileId,
  categoryOf: (item: T) => Category,
  cycleOf: (item: T) => number,
): OutlineItem<T>[] {
  const out: OutlineItem<T>[] = [];
  for (const entry of groupSteps(items, profile, categoryOf)) {
    if (entry.type === "item") {
      out.push(entry);
      continue;
    }
    let run: T[] = [];
    for (const item of entry.items) {
      const prev = run[run.length - 1];
      if (prev !== undefined && cycleOf(prev) !== cycleOf(item)) {
        out.push({ type: "group", items: run });
        run = [];
      }
      run.push(item);
    }
    if (run.length > 0) {
      out.push({ type: "group", items: run });
    }
  }
  return out;
}
