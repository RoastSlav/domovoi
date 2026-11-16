import { useEffect, useRef } from "react";
import type { Action } from "../../api/types";
import { useToast } from "../../components";

const IN_FLIGHT = new Set(["approved", "dispatched"]);

/** How a finished action is announced: never as a success or a failure it did not have. */
type OutcomeKind = "success" | "error" | "info";

/** What to say when an action that was running reaches `status`; nothing for the others. */
function outcome(
  action: Action,
  host: string | undefined,
): { kind: OutcomeKind; title: string } | undefined {
  const where = host ? `on ${host}` : "on the host";
  switch (action.status) {
    case "succeeded":
      return { kind: "success", title: `${action.skill} succeeded ${where}` };
    case "verified":
      return { kind: "success", title: `${action.skill} fixed the problem ${where}` };
    case "failed":
      return { kind: "error", title: `${action.skill} failed ${where}` };
    case "verify_failed":
      return {
        kind: "error",
        title: `${action.skill} ran ${where}, but the problem is still there`,
      };
    case "rejected":
      return { kind: "error", title: `The agent ${where} refused to run ${action.skill}` };
    case "outcome_unknown":
      return { kind: "info", title: `${action.skill} result is unknown ${where}` };
    case "not_run":
      return { kind: "info", title: `${action.skill} never ran ${where}` };
    default:
      return undefined;
  }
}

/**
 * Says how an approved action ended, as a toast, when the page sees it leave "running":
 * the person who approved it is told the result wherever on the page they are looking. A
 * result that was already in when the page loaded is not announced. An unknown outcome and
 * a run that never happened are information, never a success or a failure.
 */
export function useActionOutcomes(actions: readonly Action[], hosts: Map<string, string>): void {
  const toast = useToast();
  const seen = useRef(new Map<string, string>());

  useEffect(() => {
    for (const action of actions) {
      const before = seen.current.get(action.id);
      seen.current.set(action.id, action.status);
      const wasRunning = before !== undefined && (IN_FLIGHT.has(before) || before === "succeeded");
      if (!wasRunning || before === action.status) {
        continue;
      }
      const said = outcome(action, hosts.get(action.host_id));
      if (!said) {
        continue;
      }
      const detail = action.reason ? { detail: action.reason } : {};
      if (said.kind === "success") {
        toast.success({ title: said.title });
      } else if (said.kind === "error") {
        toast.error({ title: said.title, ...detail });
      } else {
        toast.info({ title: said.title, ...detail });
      }
    }
  }, [actions, hosts, toast]);
}
