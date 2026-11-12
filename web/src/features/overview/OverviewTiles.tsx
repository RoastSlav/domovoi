import type { Host, Overview } from "../../api/types";
import { Grid, Skeleton, StatTile } from "../../components";
import { hostConnectionOf } from "../../design/tone";

/** How many hosts are up, from the hosts themselves. */
export interface Fleet {
  /** Hosts whose agent is connected now. */
  online: number;
  /** Hosts that have, or should have, an agent: the hub's own machine is not counted. */
  total: number;
  /** Enrolled agents that are not connected, or were refused. */
  offline: number;
  /** Hosts whose agent has never connected. */
  never: number;
}

/**
 * Counts the fleet from the host list: the overview's `hosts_offline` only knows enrolled
 * agents, so a host that never connected would read as fine.
 */
export function fleetOf(hosts: readonly Host[]): Fleet {
  const fleet: Fleet = { online: 0, total: 0, offline: 0, never: 0 };
  for (const host of hosts) {
    const connection = hostConnectionOf(host);
    if (connection === "hub_only") {
      continue;
    }
    fleet.total += 1;
    if (connection === "connected") {
      fleet.online += 1;
    } else if (connection === "never_connected") {
      fleet.never += 1;
    } else {
      fleet.offline += 1;
    }
  }
  return fleet;
}

/** "1 offline, 1 never connected", or "All connected". */
function fleetHint(fleet: Fleet): string {
  const parts = [
    fleet.offline > 0 ? `${fleet.offline} offline` : "",
    fleet.never > 0 ? `${fleet.never} never connected` : "",
  ].filter(Boolean);
  if (parts.length > 0) {
    return parts.join(", ");
  }
  return fleet.total === 0 ? "No agents yet" : "All connected";
}

/**
 * The four numbers that answer "is anything wrong?": what needs the person, what is open,
 * what waits for approval and how many hosts are up. Each tile leads to its list.
 */
export function OverviewTiles({ overview, hosts }: { hosts: readonly Host[]; overview: Overview }) {
  const needs = overview.incidents_action_needed;
  const waiting = overview.approvals_waiting;
  const fleet = fleetOf(hosts);
  const down = fleet.offline + fleet.never;
  return (
    <Grid min="xs" gap={3} fill>
      <StatTile
        label="Needs attention"
        value={needs}
        tone={needs > 0 ? "danger" : "success"}
        hint={needs > 0 ? "Open and nobody has taken them" : "All clear"}
        to="/incidents"
      />
      <StatTile label="Open incidents" value={overview.incidents_open} to="/incidents" />
      <StatTile
        label="Waiting for approval"
        value={waiting}
        tone={waiting > 0 ? "pending" : undefined}
        hint={waiting > 0 ? "Nothing runs until you decide" : "Nothing to decide"}
        to="/approvals"
      />
      <StatTile
        label="Hosts online"
        value={`${fleet.online} of ${fleet.total}`}
        tone={down > 0 ? "danger" : fleet.total > 0 ? "success" : undefined}
        hint={fleetHint(fleet)}
        to="/hosts"
      />
    </Grid>
  );
}

/** Four tiles' worth of placeholder, so the page does not jump when the counts arrive. */
export function OverviewTilesSkeleton() {
  return (
    <Grid min="xs" gap={3} fill>
      {[0, 1, 2, 3].map((n) => (
        <Skeleton key={n} shape="block" />
      ))}
    </Grid>
  );
}
