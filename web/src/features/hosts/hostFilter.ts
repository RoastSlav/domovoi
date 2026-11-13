import type { Host } from "../../api/types";
import { hostConnectionOf } from "../../design/tone";

/** Which hosts the list shows: everything, the ones that need a look, or by connection. */
export type HostFilter = "all" | "problems" | "online" | "offline";

/** The search box appears when there are more hosts than this. */
export const SEARCH_FROM = 8;

/** Whether the host's agent is not reporting: offline, incompatible or never connected. */
export function isOffline(host: Host): boolean {
  if (host.kind === "external") {
    return false;
  }
  const connection = hostConnectionOf(host);
  return (
    connection === "offline" ||
    connection === "incompatible" ||
    connection === "expected_incompatible" ||
    connection === "never_connected"
  );
}

/** Whether the host needs a person: it is offline, incompatible, never connected or out of date. */
export function isProblem(host: Host): boolean {
  return isOffline(host) || (host.connected && host.outdated);
}

const MATCHES: Record<HostFilter, (host: Host) => boolean> = {
  all: () => true,
  problems: isProblem,
  online: (host) => host.connected,
  offline: isOffline,
};

/** How many of `hosts` each filter would show. */
export function countsOf(hosts: readonly Host[]): Record<HostFilter, number> {
  return {
    all: hosts.length,
    problems: hosts.filter(isProblem).length,
    online: hosts.filter(MATCHES.online).length,
    offline: hosts.filter(isOffline).length,
  };
}

/** The hosts that match `filter` and whose name contains `query`, ignoring case. */
export function filterHosts(hosts: readonly Host[], filter: HostFilter, query: string): Host[] {
  const needle = query.trim().toLowerCase();
  return hosts.filter(
    (host) => MATCHES[filter](host) && (needle === "" || host.name.toLowerCase().includes(needle)),
  );
}
