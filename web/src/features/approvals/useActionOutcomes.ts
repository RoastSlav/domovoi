import { useEffect, useRef } from "react";
import type { Action } from "../../api/types";
import { useToast } from "../../components";

const IN_FLIGHT = new Set(["approved", "dispatched"]);

/** What to say when an action that was running reaches `status`; nothing for the others. */
function outcome(
  action: Action,
  host: string | undefined,
): { failed: boolean; title: string } | undefined {
  const where = host ? `on ${host}` : "on the host";
  switch (action.status) {
    case "succeeded":
      return { failed: false, title: `${action.skill} succeeded ${where}` };
    case "verified":
      return { failed: false, title: `${action.skill} fixed the problem ${where}` };
    case "failed":
      return { failed: true, title: `${action.skill} failed ${where}` };
    case "verify_failed":
      return {
        failed: true,
        title: `${action.skill} ran ${where}, but the problem is still there`,
      };
    case "rejected":
      return { failed: true, title: `The agent ${where} refused to run ${action.skill}` };
    default:
      return undefined;
  }
}

/**
 * Says how an approved action ended, as a toast, when the page sees it leave "running":
 * the person who approved it is told the result wherever on the page they are looking. A
 * result that was already in when the page loaded is not announced.
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
      if (said.failed) {
        toast.error({ title: said.title, ...(action.reason ? { detail: action.reason } : {}) });
      } else {
        toast.success({ title: said.title });
      }
    }
  }, [actions, hosts, toast]);
}
