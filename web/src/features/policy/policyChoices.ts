import { useQueries } from "@tanstack/react-query";
import { capabilitiesOptions } from "../../api/hooks/capabilities";
import { hostOptions, useHosts } from "../../api/hooks/hosts";
import {
  categoriesOf,
  containerNamesOf,
  mergeActions,
  mergeNames,
  type ActionChoice,
  type CategoryChoice,
} from "./choiceHelpers";

export type { ActionChoice, CategoryChoice } from "./choiceHelpers";

/** What a rule's pickers may offer, read from the hosts' own inventories. */
export interface PolicyChoices {
  actions: ActionChoice[];
  categories: CategoryChoice[];
  /** True when nothing could be read and the host list or every inventory failed. */
  failed: boolean;
  loading: boolean;
  /** Known target names; `kindHint` is an action's target kind, such as `container`. */
  targets: (kindHint?: string) => string[];
}

/**
 * The actions, groups and target names a rule on a host may name.
 *
 * With a host id it reads that host; with an empty id it reads every host and merges the
 * answers, so a rule for every host can name what any host offers.
 * @param hostId The rule's host id; empty means every host.
 */
export function usePolicyChoices(hostId: string): PolicyChoices {
  const hosts = useHosts(false);
  const names = (hosts.data ?? [])
    .filter((h) => hostId === "" || h.id === hostId)
    .map((h) => h.name);
  const caps = useQueries({ queries: names.map((name) => capabilitiesOptions(name)) });
  const details = useQueries({ queries: names.map((name) => hostOptions(name)) });

  const actions = mergeActions(caps.flatMap((q) => (q.data ? [q.data.entries] : [])));
  const containers = mergeNames(
    details.flatMap((q) => (q.data ? [containerNamesOf(q.data.facts)] : [])),
  );
  const loading = hosts.isPending || caps.some((q) => q.isPending && q.fetchStatus !== "idle");
  const failed =
    (hosts.isError && !hosts.data) ||
    (actions.length === 0 && caps.length > 0 && caps.every((q) => q.isError));

  return {
    actions,
    categories: categoriesOf(actions),
    failed,
    loading,
    targets: (kindHint) =>
      kindHint === undefined || kindHint === "" || kindHint.includes("container") ? containers : [],
  };
}
