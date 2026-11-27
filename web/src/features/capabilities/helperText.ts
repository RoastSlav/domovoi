import type { HelperView } from "../../api/types";
import type { AttestationState } from "../../design/helperVocab";
import { formatHold } from "./limitsText";

/** Marks what the agent or an unsigned report said: it can be wrong or invented. */
export const RELAYED = "Relayed by the agent, not verified.";

/** What to do to change what the helper protects. The page has no second list to edit. */
export const EDIT_NOTE =
  "To change what is protected, edit helper.toml on the host, then Re-check. This page only shows what the helper reported.";

/** How the owner learns whether an edit of helper.toml is valid before the helper refuses it. */
export const CHECK_HELPER_TOML =
  "After editing helper.toml, check it on the host: sudo domovoi check config /etc/domovoi/helper.toml. A file with a mistake makes the helper refuse every request.";

/** Shown when a verified report names no address for the hub itself. */
export const HUB_ADDR_NOTE =
  "The report lists no address for this hub, so the helper may not protect the connection the hub uses. Add the hub's address to helper.toml on the host, then Re-check.";

/** Shown while there is no verified report: nothing on this page may be read as protection. */
export const NOT_PROTECTED =
  "Without a verified report, the hub cannot tell what the helper protects, so no path is shown as covered.";

/** How to read the received key's fingerprint on the host. */
export const FINGERPRINT_COMMAND = "domovoi helper fingerprint";

/**
 * Whether the hub holds a verified, fresh report for the host. The only place that decides it:
 * a verdict of "verified" without the report itself, or with a stale one, does not count.
 */
export function hasVerifiedReport(view: HelperView): boolean {
  return view.attestation.state === "verified" && !!view.verified && view.verified.fresh;
}

/** The attestation state to show: "verified" only when {@link hasVerifiedReport} says so. */
export function shownAttestation(view: HelperView): AttestationState {
  if (hasVerifiedReport(view)) {
    return "verified";
  }
  return view.attestation.state === "stale" ? "stale" : "unverified";
}

/** The generation and how long the report has been held, for a verified report. */
export function freshnessText(view: HelperView): string {
  const report = view.verified;
  if (!report) {
    return "";
  }
  return `Generation ${report.generation}, held for ${formatHold(report.age_seconds)}.`;
}

/** What a list of names says when it is empty. */
export function listOrNone(items: readonly string[] | undefined, none: string): string {
  return items && items.length > 0 ? items.join(", ") : none;
}

/** The hub's limits on containments the helper allows, in words. */
export function limitsText(view: HelperView): string {
  const report = view.verified;
  if (!report) {
    return "";
  }
  if (report.max_active === 0) {
    return "The helper allows no containment.";
  }
  return `Up to ${report.max_active} active at once, each for at most ${formatHold(report.max_ttl_s)}.`;
}

/** What the owner typed with spaces and case removed, as the hub compares it. */
export function normalizeFingerprint(typed: string): string {
  return typed.replace(/\s+/g, "").toLowerCase();
}
