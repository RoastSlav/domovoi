import type { Fact } from "../../api/types";
import { containersOf } from "../hosts/containerRows";

/** One action a rule can name, as a host's inventory lists it. */
export interface ActionChoice {
  category: string;
  /** The contract id, the value a rule stores. */
  contract: string;
  describe: string;
  effect: string;
  /** What kind of thing it acts on, such as `container`. */
  targetKind: string;
  title: string;
}

/** A group of actions and how many it holds. */
export interface CategoryChoice {
  category: string;
  count: number;
}

/** The inventory row fields the picker reads. */
type Entry = {
  category: string;
  contract: string;
  describe: string;
  effect: string;
  target_kind: string;
  title: string;
};

/**
 * Merges the inventories of several hosts into one list of actions.
 * @param lists One inventory per host.
 * @returns Each contract once (the first host's wording wins), sorted by title.
 */
export function mergeActions(lists: readonly (readonly Entry[])[]): ActionChoice[] {
  const seen = new Map<string, ActionChoice>();
  for (const list of lists) {
    for (const entry of list) {
      if (!seen.has(entry.contract)) {
        seen.set(entry.contract, {
          category: entry.category,
          contract: entry.contract,
          describe: entry.describe,
          effect: entry.effect,
          targetKind: entry.target_kind,
          title: entry.title,
        });
      }
    }
  }
  return [...seen.values()].sort(
    (a, b) => a.title.localeCompare(b.title) || a.contract.localeCompare(b.contract),
  );
}

/**
 * Groups actions by category.
 * @returns Each category once with its action count, sorted by category id.
 */
export function categoriesOf(actions: readonly ActionChoice[]): CategoryChoice[] {
  const counts = new Map<string, number>();
  for (const action of actions) counts.set(action.category, (counts.get(action.category) ?? 0) + 1);
  return [...counts]
    .map(([category, count]) => ({ category, count }))
    .sort((a, b) => a.category.localeCompare(b.category));
}

/**
 * A readable title for a category id: `container.lifecycle` becomes `Container: lifecycle`.
 * @param category The category id.
 */
export function categoryTitle(category: string): string {
  const [family = "", ...rest] = category.split(".");
  const head = family.charAt(0).toUpperCase() + family.slice(1).replace(/_/g, " ");
  const tail = rest.join(" ").replace(/_/g, " ");
  return tail === "" ? head : `${head}: ${tail}`;
}

/**
 * Merges lists of names.
 * @returns Each name once, sorted.
 */
export function mergeNames(lists: readonly (readonly string[])[]): string[] {
  return [...new Set(lists.flat())].sort((a, b) => a.localeCompare(b));
}

/**
 * The container names a host's facts mention.
 * @param facts One host's facts.
 */
export function containerNamesOf(facts: readonly Fact[]): string[] {
  return containersOf(facts).rows.map((row) => row.name);
}
