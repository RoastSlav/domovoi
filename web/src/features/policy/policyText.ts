/** Why a locked rule is special; shown next to every Locked Deny mark. */
export const LOCK_HELP =
  "A locked rule is an absolute prohibition. No narrower rule can override it. Changing, unlocking or removing it needs your password.";

/** What a shadow rule is. */
export const SHADOW_HELP = "A shadow rule is recorded for comparison and never acted on.";

/** Reassurance that the order of the rule list carries no meaning. */
export const ORDER_NOTE = "Order does not matter. The most specific rule wins.";

/** What a simulation over recorded inputs does. */
export const SIMULATION_RECORDED =
  "Each decision is re-evaluated with the host state, facts and ceiling recorded when it was made. Where an input was not available then, the result shows the gap.";

/** What a simulation over current inputs does. */
export const SIMULATION_CURRENT =
  "Each step is re-evaluated with today's clock and host state. This is a what-if, not a reproduction of what happened.";

/** How competing rules are resolved, in one paragraph. */
export const PRECEDENCE_HELP =
  "The most specific matching rule wins, even over a broader Deny. At equal scope, Deny wins. When two rules overlap and neither is more specific, the safer outcome wins: Deny, then Ask for approval, then Allow. A locked rule overrides all of them. Hard limits then apply on top; they are not rules.";

/**
 * Hint under a time-window field.
 * @param zone The time zone the hub reads times in.
 */
export function zoneHint(zone: string): string {
  return `Times are read in ${zone}. A window that ends before it starts runs overnight.`;
}
