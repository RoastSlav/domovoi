import { lookup, type StatusMeta, type Table } from "./status";

/**
 * The words of containment: where a block stands, what the leak self-test allows and whether
 * the drills passed. Statuses read only through `tone.ts`. The longer sentences are in
 * `containmentText.ts`, which only the containment pages load. Nothing here says "contained"
 * while an old flow may persist, or claims support: a block is a timed rule the host enforces.
 */

/** Where a block stands, as the hub records it. */
export type BlockPhase =
  | "applying"
  | "applied"
  | "confirmed"
  | "rolling_back"
  | "rolled_back"
  | "expired"
  | "lost"
  | "not_run"
  | "failed"
  | "outcome_unknown";

/** The phases of one block, in the order they happen. */
export const BLOCK_PHASES: Table<BlockPhase> = {
  applying: {
    label: "Applying",
    tone: "progress",
    icon: "clock",
    hint: "30 s at first.",
  },
  applied: {
    label: "Confirming",
    tone: "progress",
    icon: "clock",
    hint: "30 s at first.",
  },
  confirmed: {
    label: "Blocked until",
    tone: "success",
    icon: "shield",
    hint: "Until then.",
  },
  rolling_back: {
    label: "Removing",
    tone: "progress",
    icon: "clock",
    hint: "Being removed.",
  },
  rolled_back: {
    label: "Removed",
    tone: "neutral",
    icon: "check",
    hint: "Removed early.",
  },
  expired: {
    label: "Ended",
    tone: "neutral",
    icon: "clock",
    hint: "It ran out.",
  },
  lost: {
    label: "Block lost",
    tone: "danger",
    icon: "alert-circle",
    hint: "May be open.",
  },
  not_run: {
    label: "Not applied",
    tone: "neutral",
    icon: "ban",
    hint: "Nothing changed.",
  },
  failed: {
    label: "Failed",
    tone: "danger",
    icon: "x-circle",
    hint: "Check the host.",
  },
  outcome_unknown: {
    label: "Result unknown",
    tone: "warning",
    icon: "alert-circle",
    hint: "Refresh to see.",
  },
};

/** What the leak self-test lets the helper do on a host. */
export type GateState = "open" | "blocked_leak" | "blocked_untested";

/** The helper's gate: whether it may block at all, on the last answer the hub has. */
export const GATE_STATES: Table<GateState> = {
  open: {
    label: "Blocking allowed",
    tone: "success",
    icon: "check-circle",
    hint: "No leak seen.",
  },
  blocked_leak: {
    label: "Blocking off: leak",
    tone: "danger",
    icon: "alert-circle",
    hint: "A leak was seen.",
  },
  blocked_untested: {
    label: "Blocking off: untested",
    tone: "warning",
    icon: "alert-circle",
    hint: "No clear answer.",
  },
};

/** The drill suite the helper last ran on a host. */
export type DrillsState = "passed" | "failed" | "not_run";

/** Whether the drills passed; unattended blocking waits for a pass. */
export const DRILLS_STATES: Table<DrillsState> = {
  passed: {
    label: "Drills passed",
    tone: "success",
    icon: "check-circle",
    hint: "All passed.",
  },
  failed: {
    label: "Drills failed",
    tone: "danger",
    icon: "x-circle",
    hint: "One failed.",
  },
  not_run: {
    label: "Drills not run",
    tone: "warning",
    icon: "clock",
    hint: "Not run yet.",
  },
};

/** How a block is shown; `confirming` reads as `applied`: apply is done, confirm is pending. */
export function blockPhaseMeta(phase: string): StatusMeta {
  return lookup(BLOCK_PHASES, phase === "confirming" ? "applied" : phase);
}

/** How the gate is shown; an unknown word is a neutral pill. */
export function gateStateMeta(state: string): StatusMeta {
  return lookup(GATE_STATES, state);
}

/** How the drills are shown; an unknown word is a neutral pill. */
export function drillsStateMeta(state: string): StatusMeta {
  return lookup(DRILLS_STATES, state);
}

/** Every containment vocabulary, for the walk in `tone.ts`. */
export const CONTAINMENT_VOCABULARIES: {
  meta: (value: string) => StatusMeta;
  name: string;
  values: string[];
}[] = [
  { name: "Block phase", meta: blockPhaseMeta, values: Object.keys(BLOCK_PHASES) },
  { name: "Containment gate", meta: gateStateMeta, values: Object.keys(GATE_STATES) },
  { name: "Containment drills", meta: drillsStateMeta, values: Object.keys(DRILLS_STATES) },
];
