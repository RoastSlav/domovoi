/**
 * The error codes the containment endpoints add to the common ones, and what each says. Only
 * codes a person's click can bring back are here: `drills_not_passed` answers a policy, which
 * has no screen, and the tick `unverified_not_acknowledged` asks for is a disabled button here.
 */

/** One containment error code in words: the reason, and whether the hub's own text adds to it. */
export const CONTAINMENT_CATALOGUE = {
  containment_blocked: { reason: "Blocking is off.", specific: true },
  helper_unavailable: { reason: "The helper did not answer.", specific: false },
  approval_window_too_short: { reason: "Too little time is left.", specific: false },
} as const;

/** Every error code the containment endpoints return to a person beyond the common ones. */
export const CONTAINMENT_HUB_CODES = Object.keys(
  CONTAINMENT_CATALOGUE,
) as (keyof typeof CONTAINMENT_CATALOGUE)[];
