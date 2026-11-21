import type { Table } from "./status";

/**
 * The words for an Operator turn: where one answer stands. Read only through `turnStatusMeta`
 * in `tone.ts`.
 */

/** Where the answer to one question is. */
export type TurnStatus = "pending" | "complete" | "failed" | "cancelled";

export const TURN_STATUSES: Table<TurnStatus> = {
  pending: {
    label: "Answering",
    tone: "progress",
    icon: "clock",
    hint: "The Operator is writing its answer. You can leave this page; it keeps going.",
  },
  complete: {
    label: "Answered",
    tone: "success",
    icon: "check-circle",
    hint: "The answer is saved in this conversation.",
  },
  failed: {
    label: "Not answered",
    tone: "warning",
    icon: "alert",
    hint: "The Operator could not answer. Retry asks the same question again.",
  },
  cancelled: {
    label: "Stopped",
    tone: "neutral",
    icon: "x-circle",
    hint: "You stopped this answer. Retry asks the same question again.",
  },
};
