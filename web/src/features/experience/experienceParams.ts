import type { ExperienceSearchParams } from "../../api/hooks/experience";

/** Where the search looks: the selected host (default) or every host, only when asked. */
export type ExperienceScope = "host" | "global";

/** The search as it lives in the address: `?scope=global` and `?q=`. */
export interface ExperienceView {
  q: string;
  scope: ExperienceScope;
}

/** Reads the address; no `scope` (or any other value) means the selected host. */
export function readExperienceView(params: URLSearchParams): ExperienceView {
  return {
    scope: params.get("scope") === "global" ? "global" : "host",
    q: params.get("q") ?? "",
  };
}

/** The address after `change`: the defaults leave it, every other parameter stays. */
export function writeExperienceView(
  previous: URLSearchParams,
  change: Partial<ExperienceView>,
): URLSearchParams {
  const next = new URLSearchParams(previous);
  if (change.scope !== undefined) {
    if (change.scope === "global") {
      next.set("scope", "global");
    } else {
      next.delete("scope");
    }
  }
  if (change.q !== undefined) {
    if (change.q === "") {
      next.delete("q");
    } else {
      next.set("q", change.q);
    }
  }
  return next;
}

/**
 * The request for a view, or undefined when none may be made: a host search needs a selected
 * host. A global search never carries a host.
 */
export function experienceRequest(
  view: ExperienceView,
  host: string | undefined,
): ExperienceSearchParams | undefined {
  if (view.scope === "global") {
    return { q: view.q, scope: "global" };
  }
  return host === undefined ? undefined : { q: view.q, scope: "host", host };
}
