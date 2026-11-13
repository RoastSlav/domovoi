import type { Host } from "../../api/types";
import type { TableSort } from "../../components";
import { hostConnectionOf, hostKindMeta, type HostConnection } from "../../design/tone";

/** Offline hosts first: the list opens on what needs a person. */
export const DEFAULT_HOST_SORT: TableSort = { key: "connection", direction: "ascending" };

/**
 * Where a host falls in the list: trouble first (offline or incompatible, then never
 * connected, then connected but out of date), then the quiet ones. An external host has no
 * agent to be in trouble, so it sorts with the quiet ones.
 */
function rankOf(host: Host): number {
  const connection = hostConnectionOf(host);
  if (host.kind === "external" || connection === "hub_only") {
    return 4;
  }
  const RANKS: Record<HostConnection, number> = {
    offline: 0,
    incompatible: 0,
    expected_incompatible: 0,
    never_connected: 1,
    connected: host.outdated ? 2 : 3,
    hub_only: 4,
  };
  return RANKS[connection];
}

/** A time as a number; a host that is connected now is the most recent, one never seen the least. */
function seenAt(host: Host): number {
  if (host.connected) {
    return Number.POSITIVE_INFINITY;
  }
  return host.last_seen ? new Date(host.last_seen).getTime() : Number.NEGATIVE_INFINITY;
}

function compareBy(key: string, a: Host, b: Host): number {
  switch (key) {
    case "connection":
      return rankOf(a) - rankOf(b);
    case "kind":
      return hostKindMeta(a.kind).label.localeCompare(hostKindMeta(b.kind).label);
    case "version":
      return (a.version ?? "").localeCompare(b.version ?? "", "en", { numeric: true });
    case "incidents":
      return a.open_incidents - b.open_incidents;
    case "seen": {
      const [x, y] = [seenAt(a), seenAt(b)];
      return x === y ? 0 : x < y ? -1 : 1;
    }
    default:
      return a.name.localeCompare(b.name, "en", { numeric: true });
  }
}

/** `hosts` ordered by `sort`, ties broken by name so the order never jumps between refreshes. */
export function sortHosts(hosts: readonly Host[], sort: TableSort): Host[] {
  const sign = sort.direction === "ascending" ? 1 : -1;
  return [...hosts].sort(
    (a, b) =>
      sign * compareBy(sort.key, a, b) || a.name.localeCompare(b.name, "en", { numeric: true }),
  );
}

/** The sort after a click on `key`: a second click on the same column reverses it. */
export function nextSort(sort: TableSort, key: string): TableSort {
  return {
    key,
    direction: sort.key === key && sort.direction === "ascending" ? "descending" : "ascending",
  };
}
