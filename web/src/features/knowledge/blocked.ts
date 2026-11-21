/** What a host's management paths mean for unattended containment, as words. */
export interface BlockedSummary {
  blocked: boolean;
  reasons: string[];
}

function count(n: number, one: string, many: string): string | undefined {
  if (n === 0) {
    return undefined;
  }
  return n === 1 ? `1 ${one}` : `${n} ${many}`;
}

/**
 * Whether unattended containment is blocked on a host, and why. It is a display of a
 * conclusion: the hub enforces. Not blocked only when coverage can be checked, there is at
 * least one path and every path is confirmed.
 */
export function containmentBlock(input: {
  coverageAvailable: boolean;
  states: readonly string[];
}): BlockedSummary {
  const tally = (state: string) => input.states.filter((s) => s === state).length;
  const known = ["candidate", "unconfirmed", "stale", "confirmed"];
  const reasons: (string | undefined)[] = [
    input.coverageAvailable ? undefined : "The helper's coverage cannot be verified yet.",
    input.states.length === 0 ? "No management paths are recorded for this host." : undefined,
    count(
      tally("candidate"),
      "path found by Domovoi is not accepted by you yet.",
      "paths found by Domovoi are not accepted by you yet.",
    ),
    count(tally("unconfirmed"), "path is not confirmed.", "paths are not confirmed."),
    count(tally("stale"), "path is stale.", "paths are stale."),
    count(
      input.states.filter((s) => !known.includes(s)).length,
      "path is in a state this page does not know.",
      "paths are in a state this page does not know.",
    ),
  ];
  const list = reasons.filter((reason): reason is string => reason !== undefined);
  return { blocked: list.length > 0, reasons: list };
}
