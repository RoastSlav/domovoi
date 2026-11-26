import type { StepperStep } from "../../components";
import type { StepState } from "../../design/status";

/** The contract a containment step runs. */
export const CONTAINMENT_CONTRACT = "firewall.block_source";

/** The phases that end a block, whichever way it ended. */
const ENDED = new Set(["rolled_back", "expired", "lost", "not_run", "failed", "outcome_unknown"]);

/** Whether a block has ended, as opposed to standing or being on its way. */
export function isOver(phase: string): boolean {
  return ENDED.has(phase);
}

/** How the last stop of the timeline reads for each phase. */
function lastLabel(phase: string, until: string | null): string {
  switch (phase) {
    case "confirmed":
      return until === null ? "Blocked" : `Blocked until ${until}`;
    case "rolling_back":
      return "Removing";
    case "rolled_back":
      return "Removed";
    case "expired":
      return "Ended";
    case "lost":
      return "Lost";
    case "not_run":
      return "Not applied";
    case "failed":
      return "Failed";
    case "outcome_unknown":
      return "Result unknown";
    default:
      return "Blocked";
  }
}

function applyState(phase: string): StepState {
  if (phase === "applying") {
    return "current";
  }
  return phase === "proposed" ? "upcoming" : "done";
}

function confirmState(phase: string): StepState {
  if (phase === "applied" || phase === "confirming") {
    return "current";
  }
  return phase === "confirmed" ? "done" : "upcoming";
}

function lastState(phase: string): StepState {
  if (phase === "confirmed" || phase === "rolling_back") {
    return "current";
  }
  return isOver(phase) ? "done" : "upcoming";
}

/**
 * The stops of one block: apply, confirm, then where it stands. Before dispatch (`proposed`)
 * nothing is done; a block that ended early shows what had happened before it ended.
 */
export function phaseSteps(phase: string, until: string | null): StepperStep[] {
  return [
    { id: "apply", label: "Apply for 30 s", state: applyState(phase) },
    { id: "confirm", label: "Confirm", state: confirmState(phase) },
    { id: "last", label: lastLabel(phase, until), state: lastState(phase) },
  ];
}
