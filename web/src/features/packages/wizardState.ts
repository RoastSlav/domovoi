import type { Host } from "../../api/types";

/** The three steps the wizard shows before the confirm dialog. */
export type Step = "choose" | "hosts" | "review";

/** What the person has chosen so far. Kept in memory and in the address, never stored. */
export interface WizardState {
  hostIds: string[];
  kind: "install" | "uninstall";
  name: string;
  step: Step;
  version: string;
}

/** The most hosts one install may name. */
export const MAX_HOSTS = 16;

/**
 * Where the wizard starts, from the address (`kind`, `package`, `version`, `host`) and the
 * selected host. Opening the page never sends anything: this only reads parameters.
 */
export function initialState(
  params: URLSearchParams,
  hosts: Host[],
  selected: string | undefined,
): WizardState {
  const name = params.get("package") ?? "";
  const version = params.get("version") ?? "";
  const wanted = params.get("host") ?? selected;
  const host = wanted === undefined ? undefined : hosts.find((item) => item.name === wanted);
  return {
    step: name !== "" && version !== "" ? "hosts" : "choose",
    kind: params.get("kind") === "uninstall" ? "uninstall" : "install",
    name,
    version,
    hostIds: host ? [host.id] : [],
  };
}

/** Whether the current step may go on; the reason feeds a button's `disabledReason`. */
export function canContinue(state: WizardState): { ok: true } | { ok: false; reason: string } {
  if (state.step === "choose" && (state.name === "" || state.version === "")) {
    return { ok: false, reason: "Choose a package first." };
  }
  if (state.step === "hosts") {
    if (state.hostIds.length === 0) {
      return { ok: false, reason: "Choose at least one host." };
    }
    if (state.hostIds.length > MAX_HOSTS) {
      return { ok: false, reason: `Choose at most ${MAX_HOSTS} hosts.` };
    }
  }
  return { ok: true };
}
