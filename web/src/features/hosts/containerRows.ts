import type { Fact } from "../../api/types";
import type { ContainerState } from "../../design/tone";

/** One container, put together from its `container.<name>.<field>` facts. */
export interface ContainerRow {
  at: string;
  image: string;
  name: string;
  ports: string;
  /** How many times it restarted, when the state says. */
  restarts: number;
  state: ContainerState;
  /** The state's first word as the engine reports it: created, paused, exited. */
  word: string;
}

const KEY = /^container\.(.+)\.(state|image|ports)$/;

/** The state text of a container as a state, a restart count and its first word. */
function readState(text: string): Pick<ContainerRow, "restarts" | "state" | "word"> {
  const word = /^[a-z]+/i.exec(text.trim())?.[0]?.toLowerCase() ?? "";
  const restarts = Number(/restarted (\d+) times?/i.exec(text)?.[1] ?? 0);
  let state: ContainerState = "other";
  if (word === "restarting") {
    state = "restarting";
  } else if (word === "exited" || word === "dead") {
    state = "exited";
  } else if (word === "running") {
    if (/\(unhealthy\)/i.test(text)) {
      state = "unhealthy";
    } else {
      state = /\(healthy\)/i.test(text) ? "healthy" : "running";
    }
  }
  return { state, restarts, word };
}

/** Whether a state needs a person: such containers come first in the table. */
export function isProblemState(state: ContainerState): boolean {
  return state === "unhealthy" || state === "restarting" || state === "exited";
}

/**
 * The containers in `facts`, problems first and then by name, and the keys they used. A
 * `container.*` fact that is not a state, image or ports stays out, for the other facts.
 */
export function containersOf(facts: readonly Fact[]): { rows: ContainerRow[]; used: Set<string> } {
  const used = new Set<string>();
  const found = new Map<string, Map<string, Fact>>();
  for (const fact of facts) {
    const match = KEY.exec(fact.key);
    if (match) {
      const fields = found.get(match[1] ?? "") ?? new Map<string, Fact>();
      fields.set(match[2] ?? "", fact);
      found.set(match[1] ?? "", fields);
      used.add(fact.key);
    }
  }
  const rows = [...found].map(([name, fields]): ContainerRow => {
    const all = [...fields.values()];
    return {
      name,
      image: fields.get("image")?.value ?? "",
      ports: fields.get("ports")?.value ?? "",
      at: all.reduce((latest, fact) => (fact.observed_at > latest ? fact.observed_at : latest), ""),
      ...readState(fields.get("state")?.value ?? ""),
    };
  });
  rows.sort(
    (a, b) =>
      Number(isProblemState(b.state)) - Number(isProblemState(a.state)) ||
      a.name.localeCompare(b.name),
  );
  return { rows, used };
}
