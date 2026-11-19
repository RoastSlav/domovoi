import type { PauseReason, TraceStep } from "../../api/operationTrace";
import type { Execution, OperationSummary } from "../../api/types";
import { budgetLabel } from "../../design/operationText";
import type { Tone } from "../../design/tone";

/** A button a banner offers. */
export type BannerAction = "show_step" | "resume" | "raise_limit" | "review" | "cancel";

/** The one banner an operation shows. */
export interface BannerSpec {
  key: string;
  tone: Tone;
  title: string;
  body: string;
  actions: BannerAction[];
  /** For `show_step`: the step to select. */
  step?: string;
}

/** The slice of an operation the banner reads (a summary or the trace's operation). */
type BannerOperation = Pick<OperationSummary, "status" | "hosts"> & {
  pause_reason?: PauseReason | null;
};

function hostList(operation: BannerOperation, hostNames: Map<string, string>): string {
  const names = operation.hosts.map((host) => hostNames.get(host) ?? host);
  return names.length > 0 ? names.join(", ") : "the host";
}

function paused(
  operation: BannerOperation,
  steps: readonly TraceStep[],
  executions: readonly Execution[],
  hostNames: Map<string, string>,
): BannerSpec {
  const reason = operation.pause_reason;
  switch (reason?.type) {
    case "outcome_unknown": {
      const step = steps.find((candidate) => candidate.id === reason.step);
      const execution = executions.find((candidate) => candidate.step === reason.step);
      const host = execution
        ? (hostNames.get(execution.host_id) ?? execution.host_id)
        : hostList(operation, hostNames);
      return {
        key: "outcome_unknown",
        tone: "warning",
        title: `Result unknown: ${step?.summary ?? "an action"}`,
        body: `It may have run on ${host}. Check the host before continuing, then resume or cancel.`,
        actions: ["show_step", "resume", "cancel"],
        ...(reason.step ? { step: reason.step } : {}),
      };
    }
    case "budget":
      return {
        key: "budget",
        tone: "warning",
        title: `Paused: the ${budgetLabel(reason.exhausted ?? "").toLowerCase()} limit is spent`,
        body: "Raise the limit to let it continue, or cancel it.",
        actions: ["raise_limit", "cancel"],
      };
    case "error":
      return {
        key: "error",
        tone: "danger",
        title: "Paused after an error",
        body: reason.message ?? "",
        actions: ["resume", "cancel"],
      };
    case "manual":
      return {
        key: "manual",
        tone: "info",
        title: "Paused by a person",
        body: "Nothing runs until you resume it.",
        actions: ["resume", "cancel"],
      };
    default: {
      const unknown = reason && reason.type !== "global_pause" ? ` (${reason.type})` : "";
      return {
        key: "paused",
        tone: "warning",
        title: "Paused",
        body: `This pause is lifted somewhere else. Resume is not available here.${unknown}`,
        actions: ["cancel"],
      };
    }
  }
}

/** The banner for the state that needs attention, or null. The first matching row of the priority table wins. */
export function operationBanner(
  operation: BannerOperation,
  steps: readonly TraceStep[],
  executions: readonly Execution[],
  hostNames: Map<string, string>,
): BannerSpec | null {
  switch (operation.status) {
    case "paused":
      return paused(operation, steps, executions, hostNames);
    case "waiting_approval": {
      const n = executions.filter((execution) => execution.state === "awaiting_approval").length;
      return {
        key: "waiting_approval",
        tone: "pending",
        title: "Waiting for your approval",
        body: `${n === 1 ? "1 action waits" : `${n} actions wait`} below for you. Nothing runs until you approve.`,
        actions: ["review"],
      };
    }
    case "waiting_user":
      return {
        key: "waiting_user",
        tone: "pending",
        title: "Waiting for your answer",
        body: "Domovoi asked a question below.",
        actions: ["review"],
      };
    case "waiting_agent":
      return {
        key: "waiting_agent",
        tone: "info",
        title: `Waiting for ${hostList(operation, hostNames)}`,
        body: "Its agent is not connected. The operation continues when it returns; if it stays away too long the step is marked unavailable.",
        actions: [],
      };
    case "failed":
      return {
        key: "failed",
        tone: "danger",
        title: "The operation failed",
        body: "It stopped without reaching its goal. The steps below show where.",
        actions: [],
      };
    case "escalated":
      return {
        key: "escalated",
        tone: "warning",
        title: "Domovoi handed this to you",
        body: "It stopped trying. The steps below show what it found and did.",
        actions: [],
      };
    default:
      return null;
  }
}
