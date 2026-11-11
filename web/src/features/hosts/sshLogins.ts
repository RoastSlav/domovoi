import type { IncidentSummary } from "../../api/types";
import type { SshLogin } from "../../design/tone";

/** One row of the login history: a user's logins from one address. */
export interface LoginEntry {
  /** The client address, empty when the incident does not name one. */
  address: string;
  at: string;
  id: string;
  /** `lan` or `wan` as the agent judged the address, empty when not named. */
  network: string;
  /** How many logins the row stands for. */
  count: number;
  /** How the attempt ended; a failed one may count as `many_failures` through {@link resultOf}. */
  result: "accepted" | "failed";
  user: string;
}

/** From this many failed logins from one address on, they read as someone guessing. */
export const MANY_FAILURES = 5;

const TITLE = /^SSH login for (\S+) from (\S+) \((\w+)\)$/;

/**
 * The login history of a host as its incidents tell it: the agent raises one incident per
 * user for the successful logins it saw, titled with the address and network. Failed
 * attempts are not reported by the agent, so every entry here is an accepted login.
 */
export function loginsOf(incidents: readonly IncidentSummary[]): LoginEntry[] {
  return incidents.map((incident) => {
    const [, user, address, network] = TITLE.exec(incident.title) ?? [];
    return {
      id: incident.id,
      user: incident.target ?? user ?? "unknown user",
      address: address ?? "",
      network: network ?? "",
      at: incident.last_firing_at,
      count: incident.occurrences,
      result: "accepted",
    };
  });
}

/**
 * The result of `entry` in the history `all`: accepted, failed, or `many_failures` for a
 * failed login from an address that failed {@link MANY_FAILURES} times or more.
 */
export function resultOf(entry: LoginEntry, all: readonly LoginEntry[]): SshLogin {
  if (entry.result === "accepted") {
    return "accepted";
  }
  const failures = all
    .filter((other) => other.result === "failed" && other.address === entry.address)
    .reduce((total, other) => total + other.count, 0);
  return failures >= MANY_FAILURES ? "many_failures" : "failed";
}

/** What the history is narrowed to: how the attempt ended and where it came from. */
export interface LoginFilter {
  network: "all" | "lan" | "wan";
  result: "all" | "accepted" | "failed";
}

/** The history with no narrowing. */
export const ALL_LOGINS: LoginFilter = { result: "all", network: "all" };

/** The entries that match `filter`; many failures count as failed. */
export function filterLogins(entries: readonly LoginEntry[], filter: LoginFilter): LoginEntry[] {
  return entries.filter(
    (entry) =>
      (filter.result === "all" || entry.result === filter.result) &&
      (filter.network === "all" || entry.network === filter.network),
  );
}
