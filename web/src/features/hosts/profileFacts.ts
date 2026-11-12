/** What the profile's summary says about the machine, read from the hub's summary lines. */
export interface ProfileFacts {
  agent?: string;
  arch?: string;
  /** Names of the checks the hub runs on this host. */
  checks: string[];
  /** Recent changes since the previous profile, one per line. */
  changes: string[];
  containers?: { running: number; total: number };
  cpus?: string;
  kernel?: string;
  memory?: string;
  os?: string;
  /** Names of the read skills the hub may run on its own. */
  skills: string[];
}

/** A list line such as `checks: disk, memory` as its names; `undefined` for any other line. */
function names(line: string, noun: string): string[] | undefined {
  const prefix = `${noun}: `;
  return line.startsWith(prefix)
    ? line
        .slice(prefix.length)
        .split(", ")
        .map((name) => name.trim())
        .filter(Boolean)
    : undefined;
}

/** True when the summary named at least one fact, so the page can show it as facts. */
export function hasFacts(facts: ProfileFacts): boolean {
  return Boolean(
    facts.agent || facts.os || facts.cpus || facts.memory || facts.containers || facts.changes[0],
  );
}

/**
 * Reads the summary of a profile note (the part before its details) into facts. A line it
 * does not know is ignored; the page falls back to the raw text when nothing was found.
 */
export function readFacts(summary: string): ProfileFacts {
  const facts: ProfileFacts = { checks: [], changes: [], skills: [] };
  let changes = false;
  for (const raw of summary.split("\n")) {
    const line = raw.trim();
    if (line.startsWith("#")) {
      changes = /^#+\s*Recent changes/i.test(line);
      continue;
    }
    if (changes) {
      if (line.startsWith("- ")) {
        facts.changes.push(line.slice(2));
      }
      continue;
    }
    const agent = /^Agent version (.+?)\.?$/.exec(line);
    const system = /^(.+) on (\S+), kernel (.+)$/.exec(line);
    const cpu = /^(\d+) logical CPUs?, (\d+) GiB memory$/.exec(line);
    const containers = /^(\d+) containers? \((\d+) running\)/.exec(line);
    if (agent) {
      facts.agent = agent[1] ?? "";
    } else if (system) {
      facts.os = system[1] ?? "";
      facts.arch = system[2] ?? "";
      facts.kernel = system[3] ?? "";
    } else if (cpu) {
      facts.cpus = cpu[1] ?? "";
      facts.memory = `${cpu[2] ?? ""} GiB`;
    } else if (containers) {
      facts.containers = { total: Number(containers[1]), running: Number(containers[2]) };
    } else if (line === "no containers") {
      facts.containers = { total: 0, running: 0 };
    } else {
      facts.skills.push(...(names(line, "read skills") ?? []));
      facts.checks.push(...(names(line, "checks") ?? []));
    }
  }
  return facts;
}

/** The lines of tool output as label and value pairs, or `undefined` when it is free text. */
export function keyValues(text: string): { label: string; value: string }[] | undefined {
  const lines = text.split("\n").filter((line) => line.trim() !== "");
  const pairs = lines.map((line) => /^\s*([^:=]{1,40}?)\s*[:=]\s+(\S.*)$/.exec(line));
  if (lines.length < 2 || pairs.some((pair) => pair === null)) {
    return undefined;
  }
  const found = pairs.map((pair) => ({ label: pair?.[1] ?? "", value: pair?.[2] ?? "" }));
  return new Set(found.map((pair) => pair.label)).size === found.length ? found : undefined;
}
