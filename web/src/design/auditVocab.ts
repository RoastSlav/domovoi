import { lookup, type ChainStatus, type LegacyStatus, type StatusMeta, type Table } from "./status";

/**
 * The two parts of the audit log fail differently, so each has its own words. The lookups
 * live here, not in `tone.ts`, to keep that file short; `tone.ts` re-exports them.
 */

/** The legacy history: entries written before the hash chain began. */
export const LEGACY_STATUSES: Table<LegacyStatus> = {
  verified: {
    label: "Verified",
    tone: "success",
    icon: "check-circle",
    hint: "The entries from before the chain still match the digest recorded when the chain began.",
  },
  count_only: {
    label: "Count matches",
    tone: "info",
    icon: "info",
    hint: "Only the number of older entries was checked. Check the whole log to compare their digest.",
  },
  mismatch: {
    label: "Changed",
    tone: "danger",
    icon: "alert",
    hint: "The older entries no longer match what was recorded. The check cannot say which entry changed.",
  },
  not_verifiable: {
    label: "Not verifiable",
    tone: "warning",
    icon: "alert",
    hint: "Older entries were pruned, so their digest can no longer be recomputed.",
  },
};

/** The chained history: every entry since the chain began, each linked to the one before. */
export const CHAIN_STATUSES: Table<ChainStatus> = {
  empty: {
    label: "No chained entries yet",
    tone: "neutral",
    icon: "clock",
    hint: "Nothing has been recorded since the chain began. New entries are chained as they are written.",
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

/** How the legacy history checked out; a value this version does not know is neutral. */
export function legacyStatusMeta(value: string): StatusMeta {
  return lookup(LEGACY_STATUSES, value);
}

/** How the chained history checked out; a value this version does not know is neutral. */
export function chainStatusMeta(value: string): StatusMeta {
  return lookup(CHAIN_STATUSES, value);
}

/** Both audit vocabularies, for the walk in `tone.ts`. */
export const AUDIT_VOCABULARIES: {
  meta: (value: string) => StatusMeta;
  name: string;
  values: string[];
}[] = [
  { name: "Audit legacy history", meta: legacyStatusMeta, values: Object.keys(LEGACY_STATUSES) },
  { name: "Audit chained history", meta: chainStatusMeta, values: Object.keys(CHAIN_STATUSES) },
];
