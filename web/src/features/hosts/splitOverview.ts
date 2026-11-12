import type { Fact } from "../../api/types";
import { containersOf, type ContainerRow } from "./containerRows";
import { disksOf, type DiskRow, type StatusLine } from "./diskRows";
import { healthOf, type HealthTile } from "./healthTiles";

/** A host's facts sorted into what the overview shows for each part of the machine. */
export interface Overview {
  containers: ContainerRow[];
  disks: DiskRow[];
  health: HealthTile[];
  /** Facts about the network: link, gateway, probes. */
  network: Fact[];
  /** Everything that fits no part above. */
  other: Fact[];
  storage: StatusLine[];
}

/** Keys that start a network fact. */
const NETWORK = /^(net|network|probe|gateway)\./;

/** Sorts `facts` into the parts of the overview; every fact lands in exactly one. */
export function splitOverview(facts: readonly Fact[]): Overview {
  const health = healthOf(facts);
  const disks = disksOf(facts);
  const containers = containersOf(facts);
  const used = new Set([...health.used, ...disks.used, ...containers.used]);
  const rest = facts.filter((fact) => !used.has(fact.key));
  return {
    health: health.tiles,
    disks: disks.rows,
    storage: disks.lines,
    containers: containers.rows,
    network: rest.filter((fact) => NETWORK.test(fact.key)),
    other: rest.filter((fact) => !NETWORK.test(fact.key)),
  };
}
