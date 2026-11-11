import type { Fact } from "../../api/types";

/** A card of facts about one part of a machine. */
export interface FactGroup {
  facts: Fact[];
  id: string;
  title: string;
}

const GROUPS: { id: string; prefixes: string[]; title: string }[] = [
  {
    id: "system",
    title: "System",
    prefixes: ["system.", "temperature.", "power.", "cpu.", "load."],
  },
  { id: "disk", title: "Disk", prefixes: ["disk.", "smart.", "zfs.", "raid.", "mdadm."] },
  { id: "memory", title: "Memory", prefixes: ["memory.", "swap."] },
  { id: "network", title: "Network", prefixes: ["net.", "network.", "probe.", "gateway."] },
];

/**
 * Splits a host's facts into cards by what they describe, by the first part of their key.
 * Facts that match none go in a last "Other" card; empty cards are left out.
 */
export function groupFacts(facts: readonly Fact[]): FactGroup[] {
  const groups: FactGroup[] = GROUPS.map(({ id, title }) => ({ id, title, facts: [] }));
  const other: FactGroup = { id: "other", title: "Other", facts: [] };
  for (const fact of facts) {
    const at = GROUPS.findIndex((group) => group.prefixes.some((p) => fact.key.startsWith(p)));
    (groups[at] ?? other).facts.push(fact);
  }
  return [...groups, other].filter((group) => group.facts.length > 0);
}
