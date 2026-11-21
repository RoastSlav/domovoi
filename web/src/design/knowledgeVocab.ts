import type { CoverageState, KnowledgeStatus, PathState, Table } from "./status";

/** The vocabularies of what Domovoi knows: fact status, management path state and coverage. */

/** The effective status of a fact or a relation. */
export const KNOWLEDGE_STATUSES: Table<KnowledgeStatus> = {
  current: {
    label: "Current",
    tone: "success",
    hint: "Seen or confirmed within its freshness period. It is a hint for you and for Domovoi, never permission.",
    icon: "check-circle",
  },
  stale: {
    label: "Stale",
    tone: "warning",
    hint: "Past its freshness period, so it may no longer be true. Verify it, or let a new observation refresh it.",
    icon: "clock",
  },
  disputed: {
    label: "Disputed",
    tone: "pending",
    hint: "Two sources give different values. Choose the one that is right.",
    icon: "alert-circle",
  },
  superseded: {
    label: "Replaced",
    tone: "neutral",
    hint: "A newer value replaced this one. It stays as history.",
    icon: "skip",
  },
  retracted: {
    label: "Retracted",
    tone: "neutral",
    hint: "Withdrawn. It is not used anywhere, and its content is removed after the retention period.",
    icon: "ban",
  },
};

/** The state of a management path of a host. */
export const PATH_STATES: Table<PathState> = {
  candidate: {
    label: "Not yours yet",
    tone: "info",
    hint: "Domovoi found or inferred this path. It is not used until you accept it.",
    icon: "help",
  },
  unconfirmed: {
    label: "Waiting for you",
    tone: "pending",
    hint: "You entered it but have not confirmed it. Confirming needs the helper's verified coverage.",
    icon: "clock",
  },
  confirmed: {
    label: "Confirmed",
    tone: "success",
    hint: "You confirmed it. This says nothing about what the helper covers now.",
    icon: "check-circle",
  },
  stale: {
    label: "Stale",
    tone: "warning",
    hint: "It is past its freshness period, or the network changed. Confirm it again.",
    icon: "clock",
  },
};

/** Whether the helper's coverage can be checked. There is deliberately no "covered" state. */
export const COVERAGE_STATES: Table<CoverageState> = {
  unavailable: {
    label: "Cannot be verified yet",
    tone: "warning",
    hint: "The check that shows the helper protects a path is not available in this version. Paths can be listed and accepted, but not confirmed.",
    icon: "alert-circle",
  },
  checkable: {
    label: "Checked when you confirm",
    tone: "info",
    hint: "When you confirm a path, the hub asks the helper whether it covers it. This page does not show the answer in advance.",
    icon: "info",
  },
};
