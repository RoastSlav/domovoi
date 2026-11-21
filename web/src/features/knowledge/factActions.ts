/** Which of the owner's buttons apply to a fact. */
export interface FactActionSet {
  accept: boolean;
  /** A management path is confirmed on the paths page; the fact page points there. */
  confirmElsewhere: boolean;
  retract: boolean;
  verify: boolean;
}

const LIVE = new Set(["current", "stale", "disputed"]);

/** The actions that apply to a fact, from its effective status, source and predicate. */
export function factActions(fact: {
  content_purged: boolean;
  predicate: string;
  source: string;
  status: string;
}): FactActionSet {
  const live = LIVE.has(fact.status);
  return {
    accept: live && fact.source !== "user" && !fact.content_purged,
    verify: live && fact.predicate !== "management_path",
    retract: live,
    confirmElsewhere: live && fact.predicate === "management_path" && fact.source === "user",
  };
}
