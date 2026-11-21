import type { CeilingEntry } from "../../api/types";

/** Why the limits belong to the host, and how to change them. */
export const CEILING_READ_ONLY =
  "The host decides what may run unattended, and Domovoi shows what the host reported. It cannot change these limits. To change them, edit the [unattended] section of /etc/domovoi/agent.toml on the host and restart the agent.";

/** What a host that reported nothing means. */
export const ceilingNotReported = (host: string) =>
  `${host} has not reported any unattended limits. Until it does, Domovoi treats unattended actions on it as not allowed, and anything that changes the host needs your approval.`;

/** A hold in seconds as `1 h`, `30 min` or `90 s`; "–" when the entry sets none. */
export function formatHold(secs: number | null | undefined): string {
  if (secs === null || secs === undefined) {
    return "–";
  }
  if (secs > 0 && secs % 3600 === 0) {
    return `${secs / 3600} h`;
  }
  if (secs > 0 && secs % 60 === 0) {
    return `${secs / 60} min`;
  }
  return `${secs} s`;
}

/** An entry's targets as plain words: "Any target" or the names, comma separated. */
export function targetsText(targets: CeilingEntry["targets"]): string {
  return targets.kind === "any" ? "Any target" : targets.names.join(", ");
}

/** An entry's argument bounds as `key: value` pairs; "–" when it has none. */
export function boundsText(args: CeilingEntry["args"]): string {
  const pairs = Object.entries(args).map(([key, value]) => `${key}: ${value}`);
  return pairs.length === 0 ? "–" : pairs.join(", ");
}
