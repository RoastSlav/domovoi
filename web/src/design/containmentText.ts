import { unverifiedReasonText } from "./helperVocab";

/**
 * The longer sentences of containment: what happens to flows that were already open, the
 * bridge gap, the flood caveat and the unverified-approval texts. Only the containment pages
 * read them, so they stay out of the vocabulary `tone.ts` loads for every page.
 */

/** What the host did with connections that were already open when the block went in. */
export function existingFlowsText(flows: string | null | undefined): string | null {
  switch (flows) {
    case "cut":
      return "Connections from this source that were already open were cut.";
    case "n/a":
      return "This host keeps no offloaded connections, so the block applies to every packet.";
    case "may_persist":
      return "A connection that was already open may keep working. The block stops new ones.";
    default:
      return null;
  }
}

/** The sentence for a firewall that lets a bridged network skip the block. */
export const BRIDGE_GAP_TEXT =
  "This host bridges a network interface. Traffic that is only bridged does not pass the block.";

/** Said wherever a block is described: a flood can make it end early. */
export const FLOOD_NOTE =
  "A flood from the blocked source can delay the hub's confirmation past 30 seconds. The block then ends early: that leaves the hub reachable, but it does not stop that source.";

/** The headline of the owner's approval when coverage cannot be verified. */
export const UNVERIFIED_WARNING =
  "The hub cannot verify that this block leaves the paths you manage the host by. If the source overlaps one, you could lock yourself out.";

/** The reason coverage could not be verified, in words. */
export function unverifiedReason(reason: string | null | undefined): string {
  return unverifiedReasonText(reason);
}

/** Said under the typed-source field. */
export const TYPED_SOURCE_HINT =
  "Type the source exactly as shown. The helper still refuses a source that overlaps a management path.";

/** What each phase means for the person, in full; the status hint is only its first words. */
export function phaseNote(phase: string): string | null {
  switch (phase) {
    case "applied":
      return "The block is on for 30 seconds. If the hub cannot confirm it in time, it ends by itself.";
    case "confirmed":
      return "The helper blocks this source until the end time, then removes the block itself.";
    case "expired":
      return "It ran out. Nothing re-applies it; blocking again needs a new request and your approval.";
    case "lost":
      return "The block vanished from the host, for example when its firewall table was cleared, so this source may no longer be blocked. Check the host, then block it again.";
    case "not_run":
      return "The helper refused the block or could not apply it. Nothing changed on the host.";
    case "failed":
      return "The block did not work. Check the host's firewall before relying on it.";
    case "outcome_unknown":
      return "The hub lost contact before it heard back. Refresh the host to see whether the block is there.";
    default:
      return null;
  }
}

/** What each gate state means, in full. */
export function gateNote(state: string): string | null {
  switch (state) {
    case "open":
      return "The self-test saw no packet get through while a block was replaced on this kernel and firewall version. That is evidence from one test, not a promise.";
    case "blocked_leak":
      return "The self-test saw packets pass while a block was being replaced. The helper refuses to block anything on this host.";
    case "blocked_untested":
      return "The self-test has no clear answer for this kernel and firewall version, so the helper refuses to block. Run the test on the host.";
    default:
      return null;
  }
}

/** The sentence under the lost-block attention row. */
export const LOST_WHY =
  "The host's firewall table was cleared, so a block may be gone. Nothing re-applies it.";

/** What the owner can do about a gate that stops blocking. */
export function gateAttentionWhy(state: string): string {
  return state === "blocked_leak"
    ? "Self-test saw a leak."
    : "The self-test has no clear answer for this kernel and firewall version. The helper refuses to block here.";
}

/** Said in place of the gate and the drills before the helper was ever asked. */
export const NOT_ASKED = "The hub has not asked this host's helper yet. Refresh to see.";
