import type { Tone } from "../../design/tone";
import type { ProfilePart } from "./profileText";

/** One number read from the host overview, for a tile. */
export interface OverviewTile {
  hint?: string;
  label: string;
  tone?: Tone;
  value: string;
}

/** A disk this full is a warning, and this full is a danger. */
const DISK_WARNING = 85;
const DISK_DANGER = 95;

/** The tone of a disk that is `used` percent full; none while there is room. */
function diskTone(used: number): Tone | undefined {
  if (used >= DISK_DANGER) {
    return "danger";
  }
  return used >= DISK_WARNING ? "warning" : undefined;
}

function diskTile(mount: string, used: number, hint: string): OverviewTile {
  const tone = diskTone(used);
  return { label: `Disk ${mount}`, value: `${used}% used`, hint, ...(tone ? { tone } : {}) };
}

/** One overview line as a tile, or `undefined` when it is not a form this page knows. */
function tileOf(line: string): OverviewTile | undefined {
  let found: RegExpExecArray | null;
  if ((found = /^uptime: (?!unknown)(.+)$/.exec(line))) {
    return { label: "Uptime", value: found[1] ?? "" };
  }
  if ((found = /^load: ([\d.]+) ([\d.]+) ([\d.]+)$/.exec(line))) {
    return { label: "Load", value: found[1] ?? "", hint: `5 min ${found[2]}, 15 min ${found[3]}` };
  }
  if ((found = /^memory: (.+) total, (.+) used$/.exec(line))) {
    return { label: "Memory", value: `${found[2]} used`, hint: `of ${found[1]}` };
  }
  if ((found = /^memory: ([\d.]+) GiB available of ([\d.]+) GiB$/.exec(line))) {
    const used = (Number(found[2]) - Number(found[1])).toFixed(1);
    return { label: "Memory", value: `${used} GiB used`, hint: `of ${found[2]} GiB` };
  }
  if ((found = /^swap: (.+) used of (.+)$/.exec(line))) {
    return { label: "Swap", value: `${found[1]} used`, hint: `of ${found[2]}` };
  }
  if ((found = /^\s+(\S+) (\S+) (\d+)%$/.exec(line))) {
    return diskTile(found[1] ?? "", Number(found[3]), `of ${found[2]}`);
  }
  if ((found = /^\s+(\S+) \(\S+\): ([\d.]+) GiB free of ([\d.]+) GiB \((\d+)%\)$/.exec(line))) {
    const hint = `${found[2]} GiB free of ${found[3]} GiB`;
    return diskTile(found[1] ?? "", 100 - Number(found[4]), hint);
  }
  return undefined;
}

/**
 * Reads the host overview tool's text (uptime, load, memory, swap, disks) into tiles. The
 * lines it cannot read come back in `rest`, so nothing the tool said is lost.
 */
export function readOverview(text: string): { rest: string; tiles: OverviewTile[] } {
  const tiles: OverviewTile[] = [];
  const rest: string[] = [];
  for (const line of text.split("\n")) {
    const tile = tileOf(line);
    if (tile) {
      tiles.push(tile);
    } else if (line.trim() !== "" && line.trim() !== "disks:") {
      rest.push(line);
    }
  }
  return { tiles, rest: tiles.length > 0 ? rest.join("\n") : text };
}

/** The columns for `count` tiles in rows of at most `max`: even rows, never a lone last tile. */
export function columnsFor(count: number, max: number): number {
  if (count <= max) {
    return Math.max(count, 1);
  }
  let cols = Math.ceil(count / Math.ceil(count / max));
  while (cols > 2 && count % cols === 1) {
    cols -= 1;
  }
  return cols;
}

/**
 * Turns the overview part into tiles. The part stays only with the lines that could not
 * become tiles, and goes when nothing is left; the other parts pass through unchanged.
 */
export function foldOverview(parts: ProfilePart[]): {
  parts: ProfilePart[];
  tiles: OverviewTile[];
} {
  const tiles: OverviewTile[] = [];
  const kept: ProfilePart[] = [];
  for (const part of parts) {
    if (!part.ok || !/overview/i.test(part.title)) {
      kept.push(part);
      continue;
    }
    const read = readOverview(part.text);
    tiles.push(...read.tiles);
    if (read.rest.trim() !== "") {
      kept.push({ ...part, text: read.rest });
    }
  }
  return { parts: kept, tiles };
}
