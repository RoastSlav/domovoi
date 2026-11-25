import { lookup, type StatusMeta, type Table } from "./status";

/** Where the hub stands on a host's helper key. */
export type HelperKeyState = "no_helper" | "not_pinned" | "pinned" | "changed";

/** The verdict on a host's helper attestation. */
export type AttestationState = "verified" | "stale" | "unverified";

/** How the hub's pin and the key it received relate: five words, never "trusted". */
export const HELPER_KEY_STATES: Table<HelperKeyState> = {
  no_helper: {
    label: "No helper key",
    tone: "neutral",
    icon: "dot",
    hint: "The hub has received no key from a helper on this host. Without a helper, containment cannot be confirmed here.",
  },
  not_pinned: {
    label: "Key not pinned",
    tone: "pending",
    icon: "clock",
    hint: "The hub received a key but you have not pinned it. Compare it with what the host prints, then pin it.",
  },
  pinned: {
    label: "Key pinned",
    tone: "success",
    icon: "lock",
    hint: "You pinned this key. Reports are checked against it; a pin alone does not make a report verified.",
  },
  changed: {
    label: "Key changed",
    tone: "danger",
    icon: "alert-circle",
    hint: "The helper now offers a different key than the one you pinned. Containment on this host is blocked until you pin the new key.",
  },
};

/** Whether the helper's signed report currently counts. Only "verified" ever reads as covered. */
export const ATTESTATION_STATES: Table<AttestationState> = {
  verified: {
    label: "Verified",
    tone: "success",
    icon: "check-circle",
    hint: "A fresh report signed by the pinned key was checked by the hub. It describes the host as the helper saw it then.",
  },
  stale: {
    label: "Attestation stale",
    tone: "warning",
    icon: "clock",
    hint: "The last verified report is older than the freshness window, so it no longer counts. Re-check.",
  },
  unverified: {
    label: "Unverified",
    tone: "warning",
    icon: "alert-circle",
    hint: "There is no valid, fresh report from the pinned key. Nothing here is protected as far as the hub can tell.",
  },
};

/** How a key state is shown; an unknown word is a neutral pill. */
export function helperKeyStateMeta(state: string): StatusMeta {
  return lookup(HELPER_KEY_STATES, state);
}

/** How an attestation state is shown; an unknown word is a neutral pill. */
export function attestationStateMeta(state: string): StatusMeta {
  return lookup(ATTESTATION_STATES, state);
}

/** Why a report does not count, one sentence per reason the hub names. */
const UNVERIFIED_REASONS: Record<string, string> = {
  key_not_pinned: "You have not pinned the helper's key yet.",
  key_mismatch: "The report is signed by a key other than the one you pinned.",
  attestation_missing: "The helper has not sent a report the hub could check.",
  attestation_stale: "The last report is older than the freshness window.",
  signature_invalid: "The report's signature did not check out against the pinned key.",
  report_incomplete: "The helper signed a report that is missing what the hub needs.",
  generation_regressed:
    "The helper's report numbers went backwards, so its state may have been rolled back.",
  path_uncovered: "A management path is not covered by the helper's verified configuration.",
  confirmation_stale:
    "A confirmed management path was last checked before the helper's state changed.",
};

/** The words for a reason an attestation is unverified; an unknown one gets a general sentence. */
export function unverifiedReasonText(reason: string | null | undefined): string {
  if (reason !== null && reason !== undefined && Object.hasOwn(UNVERIFIED_REASONS, reason)) {
    return UNVERIFIED_REASONS[reason] ?? "";
  }
  return "The hub cannot verify the helper's report right now.";
}

/** What the helper itself says is wrong, and the local action that fixes it. */
export interface HelperFault {
  /** The command to run on the host as root, when there is one. */
  command?: string;
  /** What to do, as one sentence. */
  text: string;
}

const RECOVER = "domovoi helper recover";

const HELPER_FAULTS: Record<string, HelperFault> = {
  helper_state_permissions: {
    text: "The helper's state directory has the wrong permissions. Fix the mode on the host, then re-check.",
  },
  state_missing: {
    text: "The helper's state is missing. Recover it on the host.",
    command: RECOVER,
  },
  state_corrupt: {
    text: "The helper's state is damaged. Recover it on the host.",
    command: RECOVER,
  },
  state_key_mismatch: {
    text: "The helper's state does not match its key. Recover it on the host.",
    command: RECOVER,
  },
  key_missing: { text: "The helper's key is missing. Recover it on the host.", command: RECOVER },
  uninitialized: {
    text: "The helper has not been set up. Initialise it on the host.",
    command: "domovoi helper init",
  },
  generation_regressed: {
    text: "The helper's report numbers went backwards. Recover its state on the host.",
    command: RECOVER,
  },
};

/** The local action for a token the helper or agent sent, or `undefined` when it is not one this page knows. */
export function helperFault(token: string | null | undefined): HelperFault | undefined {
  if (token !== null && token !== undefined && Object.hasOwn(HELPER_FAULTS, token)) {
    return HELPER_FAULTS[token];
  }
  return undefined;
}

/** What the agent calls the helper's state, as a plain word; relayed, never checked. */
export function helperStateWord(state: string): string {
  switch (state) {
    case "ok":
      return "Running";
    case "absent":
      return "Not installed";
    case "error":
      return "Reports an error";
    default:
      return "Unknown";
  }
}

/** Every helper vocabulary, for the walk in `tone.ts`. */
export const HELPER_VOCABULARIES: {
  meta: (value: string) => StatusMeta;
  name: string;
  values: string[];
}[] = [
  { name: "Helper key state", meta: helperKeyStateMeta, values: Object.keys(HELPER_KEY_STATES) },
  {
    name: "Helper attestation",
    meta: attestationStateMeta,
    values: Object.keys(ATTESTATION_STATES),
  },
];
