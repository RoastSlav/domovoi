/** The error codes the helper endpoints add, and what each says. */

/** Every error code the helper endpoints return beyond the common ones. */
export const HELPER_HUB_CODES = [
  "password_required",
  "wrong_password",
  "no_helper_key",
  "fingerprint_mismatch",
  "recheck_too_soon",
] as const;

/** One helper error code in words: the reason, and whether the hub's own text adds to it. */
export const HELPER_CATALOGUE: Record<
  (typeof HELPER_HUB_CODES)[number],
  { reason: string; specific: boolean }
> = {
  password_required: { reason: "Enter your password to do this.", specific: false },
  wrong_password: { reason: "That password is not right.", specific: false },
  no_helper_key: {
    reason: "The hub holds no key from this host's helper yet. Start the helper, then try again.",
    specific: false,
  },
  fingerprint_mismatch: {
    reason:
      "That is not the fingerprint of the key the hub holds. Run the command on the host again and type what it prints.",
    specific: false,
  },
  recheck_too_soon: {
    reason: "The helper was asked a moment ago. Wait ten seconds, then re-check.",
    specific: false,
  },
};
