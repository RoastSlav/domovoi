import type { Action, Report } from "../../api/types";

/**
 * Why the model proposed `action`: the reason of the report's proposal with the same skill
 * and arguments. Nothing when the report has no such proposal (or there is no report): the
 * action itself carries no reason until it fails.
 */
export function whyProposed(action: Action, report: Report | null | undefined): string | undefined {
  const args = JSON.stringify(action.args);
  return report?.proposals.find(
    (proposal) => proposal.skill === action.skill && JSON.stringify(proposal.args) === args,
  )?.reason;
}
