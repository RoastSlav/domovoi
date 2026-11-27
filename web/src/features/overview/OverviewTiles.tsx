import type { Host, Overview } from "../../api/types";
import { Grid, Skeleton, StatTile } from "../../components";
import { hostConnectionOf } from "../../design/tone";

/** How many hosts are up, from the hosts themselves. */
export interface Fleet {
  /** Hosts whose agent is connected now. */
  online: number;
  /** Hosts that have, or should have, an agent: the hub's own machine is not counted. */
  total: number;
  /** Enrolled agents that are not connected, or speak another protocol. */
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
 * The four numbers that answer "is anything wrong?": what needs the person, what is running,
 * what waits for approval and how many hosts are up. Each tile leads to its list.
 */
export function OverviewTiles({ overview, hosts }: { hosts: readonly Host[]; overview: Overview }) {
  // An older hub sends no attention count: say nothing about it rather than "all clear".
  const needs = overview.attention_total as number | undefined;
  const running = overview.operations_running ?? 0;
  const approvals = overview.operations_waiting_approval ?? 0;
  const questions = overview.operations_waiting_user ?? 0;
  const waiting = approvals + questions;
  const fleet = fleetOf(hosts);
  const down = fleet.offline + fleet.never;
  return (
    <Grid min="xs" gap={3} fill>
      <StatTile
        label="Needs you"
        value={needs ?? 0}
        tone={needs === undefined ? undefined : needs > 0 ? "danger" : "success"}
        hint={needs === 0 ? "All clear" : "Open the attention centre"}
        to="/attention"
      />
      <StatTile
        label="Running now"
        value={running}
        tone={running > 0 ? "progress" : undefined}
        hint={running > 0 ? "Operations in progress" : "Nothing running"}
        to="/operations?status=running"
      />
      <StatTile
        label="Waiting for you"
        value={waiting}
        tone={waiting > 0 ? "pending" : undefined}
        hint={
          waiting === 0
            ? "Nothing to decide"
            : questions > 0
              ? `${approvals} to approve, ${questions} ${questions === 1 ? "question" : "questions"} to answer`
              : "Nothing runs until you decide"
        }
        to="/attention"
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
