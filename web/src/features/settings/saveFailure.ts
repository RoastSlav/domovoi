import { ApiFailure, type Issue } from "../../api/types";
import { describeError } from "../../design/messages";

/** A failed save, in words, with the hub's issues and whether the settings moved on. */
export interface SaveFailure {
  detail?: string | undefined;
  issues: Issue[];
  /** The settings changed somewhere else since this page loaded them. */
  stale: boolean;
  title: string;
}

/** Turns what a save threw into words and the issues the hub listed. */
export function failureOf(error: unknown): SaveFailure {
  const described = describeError(error, "save the settings");
  const stale =
    error instanceof ApiFailure &&
    error.status === 409 &&
    error.message.includes("changed elsewhere");
  return stale
    ? {
        title: "Settings changed elsewhere",
        detail: "Reload to see the current values, then make your change again.",
        issues: [],
        stale,
      }
    : { title: described.title, detail: described.detail, issues: described.issues ?? [], stale };
}
