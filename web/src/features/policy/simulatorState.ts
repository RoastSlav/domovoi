/** Which rules a simulation runs. */
export type SimulatorRules = { source: "draft" } | { source: "version"; number: number };

/** The simulator page's state, as the address holds it. */
export interface SimulatorState {
  rules: SimulatorRules;
  selection: "enforced" | "shadow" | "both";
  inputs: "recorded" | "current";
  execution: string | null;
  item: string | null;
}

const DEFAULT: SimulatorState = {
  rules: { source: "draft" },
  selection: "both",
  inputs: "recorded",
  execution: null,
  item: null,
};

function parseRules(text: string | null, fallback: SimulatorRules): SimulatorRules {
  if (text === "draft") return { source: "draft" };
  const m = /^version-(\d+)$/.exec(text ?? "");
  return m && m[1] ? { source: "version", number: Number(m[1]) } : fallback;
}

const rulesText = (r: SimulatorRules): string =>
  r.source === "draft" ? "draft" : `version-${r.number}`;

/**
 * Reads the simulator state from the address.
 * @param params The query string.
 * @param defaults Values for anything missing or invalid.
 */
export function parseSimulatorParams(
  params: URLSearchParams,
  defaults: Partial<SimulatorState> = {},
): SimulatorState {
  const base = { ...DEFAULT, ...defaults };
  const selection = params.get("selection");
  const inputs = params.get("inputs");
  return {
    rules: parseRules(params.get("rules"), base.rules),
    selection:
      selection === "enforced" || selection === "shadow" || selection === "both"
        ? selection
        : "both",
    inputs: inputs === "recorded" || inputs === "current" ? inputs : "recorded",
    execution: params.get("execution") || base.execution,
    item: params.get("item") || base.item,
  };
}

/**
 * Writes changed simulator state into a copy of the query string.
 * @param params The current query string; left untouched.
 * @param patch The fields to set; null removes a field.
 */
export function withSimulatorParams(
  params: URLSearchParams,
  patch: Partial<SimulatorState>,
): URLSearchParams {
  const next = new URLSearchParams(params);
  const put = (key: string, value: string | null | undefined): void => {
    if (value === undefined) return;
    if (value === null || value === "") next.delete(key);
    else next.set(key, value);
  };
  if (patch.rules !== undefined) put("rules", rulesText(patch.rules));
  put("selection", patch.selection);
  put("inputs", patch.inputs);
  put("execution", patch.execution);
  put("item", patch.item);
  return next;
}
