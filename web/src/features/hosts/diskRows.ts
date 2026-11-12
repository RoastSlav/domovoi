import type { Fact } from "../../api/types";
import type { Tone } from "../../design/tone";
import {
  FULL_DANGER,
  FULL_WARNING,
  newest,
  numberOf,
  shown,
  textTone,
  toneAt,
  withUnit,
} from "./factValue";

/** One mount or pool: how full it is and how much is left. */
export interface DiskRow {
  at: string;
  /** What is left as text, `31 GiB free`. */
  free?: string;
  mount: string;
  /** Percent used, when the facts say or let it be worked out. */
  pct?: number;
}

/** One line about the state of storage: a pool's health, a RAID resync, SMART. */
export interface StatusLine {
  at: string;
  label: string;
  tone?: Tone | undefined;
  value: string;
}

/** Storage facts that are not a mount's usage belong under the disks as status lines. */
const STORAGE = /^(disk|smart|zfs|raid|mdadm)\./;
const DISK_USAGE = /^disk\.(.+)\.(used_pct|free|free_gib|free_pct|total_gib)$/;
const POOL_USAGE = /^zfs\.(.+)\.(used_pct|free_bytes)$/;
const GIB = 1024 ** 3;

/** The mount path a disk fact is about: from its key, else its label, else a guess. */
function mountOf(name: string, label: string): string {
  if (name.startsWith("/")) {
    return name;
  }
  const named = /(?:^|\s)(\/\S*)/.exec(label)?.[1];
  if (named) {
    return named;
  }
  return name === "root" ? "/" : `/${name.replaceAll("_", "/")}`;
}

interface Pending {
  facts: Fact[];
  mount: string;
}

function rowOf(entry: Pending): DiskRow {
  const get = (field: string) => entry.facts.find((fact) => fact.key.endsWith(`.${field}`));
  const [usedPct, freeGib, freePct, totalGib, freeBytes] = [
    get("used_pct"),
    get("free") ?? get("free_gib"),
    get("free_pct"),
    get("total_gib"),
    get("free_bytes"),
  ];
  let pct: number | undefined;
  if (usedPct) {
    pct = numberOf(usedPct);
  } else if (freePct) {
    pct = 100 - numberOf(freePct);
  } else if (freeGib && totalGib && numberOf(totalGib) > 0) {
    pct = (1 - numberOf(freeGib) / numberOf(totalGib)) * 100;
  }
  let free: string | undefined;
  if (freeGib) {
    free = `${withUnit(freeGib)} free`;
  } else if (freeBytes) {
    free = `${shown(numberOf(freeBytes) / GIB)} GiB free`;
  }
  return {
    mount: entry.mount,
    at: newest(entry.facts) ?? "",
    ...(pct !== undefined && !Number.isNaN(pct) ? { pct } : {}),
    ...(free ? { free } : {}),
  };
}

/**
 * The disk rows of `facts`, fullest first, and the storage status lines under them. A mount
 * with no way to tell how full it is sorts last.
 */
export function disksOf(facts: readonly Fact[]): {
  lines: StatusLine[];
  rows: DiskRow[];
  used: Set<string>;
} {
  const used = new Set<string>();
  const pending = new Map<string, Pending>();
  const lines: StatusLine[] = [];
  for (const fact of facts) {
    const disk = DISK_USAGE.exec(fact.key);
    const pool = POOL_USAGE.exec(fact.key);
    const match = disk ?? pool;
    if (match) {
      const id = `${pool ? "zfs" : "disk"}:${match[1] ?? ""}`;
      const entry = pending.get(id) ?? {
        facts: [],
        mount: pool ? `ZFS ${match[1] ?? ""}` : mountOf(match[1] ?? "", fact.label),
      };
      entry.facts.push(fact);
      pending.set(id, entry);
      used.add(fact.key);
    } else if (STORAGE.test(fact.key)) {
      const value = numberOf(fact);
      const percent = Number.isNaN(value) ? undefined : toneAt(value, FULL_WARNING, FULL_DANGER);
      used.add(fact.key);
      lines.push({
        label: fact.label,
        value: withUnit(fact),
        tone: fact.key.endsWith("_pct") ? percent : textTone(fact.value),
        at: fact.observed_at,
      });
    }
  }
  const rows = [...pending.values()]
    .map(rowOf)
    .sort((a, b) => (b.pct ?? -1) - (a.pct ?? -1) || a.mount.localeCompare(b.mount));
  return { rows, lines, used };
}

/** The tone of a disk row: a warning from 85 %, a danger from 95 %. */
export function diskTone(row: DiskRow): Tone | undefined {
  return row.pct === undefined ? undefined : toneAt(row.pct, FULL_WARNING, FULL_DANGER);
}
