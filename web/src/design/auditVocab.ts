import { lookup, type ChainStatus, type StatusMeta, type Table } from "./status";

/**
 * The audit chain has its own words. The lookup
 * lives here, not in `tone.ts`, to keep that file short; `tone.ts` re-exports them.
 */

/** The audit chain: every entry, each linked to the one before. */
export const CHAIN_STATUSES: Table<ChainStatus> = {
  empty: {
    label: "No entries yet",
    tone: "neutral",
    icon: "clock",
    hint: "Nothing has been recorded yet. New entries are chained as they are written.",
  },
  intact: {
    label: "Intact",
    tone: "success",
    icon: "check-circle",
    hint: "Every checked entry links to the one before it and its hash is correct.",
  },
  broken: {
    label: "Broken",
    tone: "danger",
    icon: "x-circle",
    hint: "A checked entry does not match the chain. Entries may have been edited, removed or added outside Domovoi.",
  },
};

/** How the audit chain checked out; a value this version does not know is neutral. */
export function chainStatusMeta(value: string): StatusMeta {
  return lookup(CHAIN_STATUSES, value);
}

/** The audit vocabulary, for the walk in `tone.ts`. */
export const AUDIT_VOCABULARIES: {
  meta: (value: string) => StatusMeta;
  name: string;
  values: string[];
}[] = [{ name: "Audit chain", meta: chainStatusMeta, values: Object.keys(CHAIN_STATUSES) }];
