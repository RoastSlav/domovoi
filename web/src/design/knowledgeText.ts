/**
 * The plain words and rules of the knowledge pages: what each predicate, source, relation
 * kind and summary field is called, and which actions need the owner's password. Pure data and
 * functions; the hub stays the authority on every rule that is repeated here.
 */

/** The label every response about past operations carries. */
export const HISTORICAL_LABEL = "historical evidence, not authorization";

/** The kinds of thing a fact can be about. */
export type SubjectKind = "host" | "container" | "unit" | "entity";

const ALL_SUBJECTS: readonly SubjectKind[] = ["host", "container", "unit", "entity"];

/** What the page needs to know about one predicate. */
export interface PredicateInfo {
  /** Whether the owner may assert it; `host_platform` is observed, never asserted. */
  assertable: boolean;
  hint: string;
  label: string;
  /** Whether a subject may hold several live values at once. */
  many: boolean;
  /** Whether a fact of this kind is used for a safety decision, so changing it asks for the password. */
  safety: boolean;
  /** The subject kinds the predicate allows. */
  subjects: readonly SubjectKind[];
}

/** The ten predicates the knowledge store knows. */
export const PREDICATES: Readonly<Record<string, PredicateInfo>> = {
  role: {
    label: "Role",
    hint: "What this thing is for.",
    subjects: ALL_SUBJECTS,
    many: false,
    safety: false,
    assertable: true,
  },
  criticality: {
    label: "Criticality",
    hint: "How much it matters: critical or warning.",
    subjects: ALL_SUBJECTS,
    many: false,
    safety: false,
    assertable: true,
  },
  owner_note: {
    label: "Note",
    hint: "Something you want Domovoi to remember.",
    subjects: ALL_SUBJECTS,
    many: true,
    safety: false,
    assertable: true,
  },
  depends_on: {
    label: "Depends on",
    hint: "Something it needs in order to work.",
    subjects: ALL_SUBJECTS,
    many: true,
    safety: false,
    assertable: true,
  },
  management_path: {
    label: "Management path",
    hint: "A network you reach this host through. Protective actions must never cut it off.",
    subjects: ["host"],
    many: true,
    safety: true,
    assertable: true,
  },
  maintenance_window: {
    label: "Maintenance window",
    hint: "A time when work on it is expected.",
    subjects: ALL_SUBJECTS,
    many: true,
    safety: false,
    assertable: true,
  },
  backup_location: {
    label: "Backed up to",
    hint: "Where its data is backed up.",
    subjects: ALL_SUBJECTS,
    many: true,
    safety: false,
    assertable: true,
  },
  never_restart: {
    label: "Never restart",
    hint: "Domovoi must not restart it.",
    subjects: ["container", "unit"],
    many: false,
    safety: false,
    assertable: true,
  },
  trusted_cidr: {
    label: "Trusted network",
    hint: "A network that is known and expected.",
    subjects: ["host"],
    many: true,
    safety: true,
    assertable: true,
  },
  host_platform: {
    label: "Platform",
    hint: "The machine's system, kernel and size, observed by Domovoi.",
    subjects: ["host"],
    many: false,
    safety: false,
    assertable: false,
  },
};

/** What is known about a predicate; an unknown one shows its raw key and allows nothing. */
export function predicateInfo(key: string): PredicateInfo {
  const known = Object.hasOwn(PREDICATES, key) ? PREDICATES[key] : undefined;
  return (
    known ?? {
      label: key,
      hint: "",
      subjects: ALL_SUBJECTS,
      many: true,
      safety: false,
      assertable: false,
    }
  );
}

/** The predicates whose facts are used for safety decisions, and so need the password to change. */
export const SAFETY_CRITICAL: ReadonlySet<string> = new Set(["management_path", "trusted_cidr"]);

/** The owner's actions on knowledge. */
export type KnowledgeAction = "assert" | "accept" | "verify" | "retract" | "confirm";

/** Whether `action` on a fact of `predicate` asks for the owner's password. */
export function needsPassword(action: KnowledgeAction, predicate: string): boolean {
  if (action === "confirm") {
    return true;
  }
  if (action === "retract") {
    return false;
  }
  return SAFETY_CRITICAL.has(predicate);
}

/** The words for where a fact came from. */
export const SOURCES: Readonly<Record<string, { hint: string; label: string }>> = {
  user: { label: "You", hint: "Entered or accepted by you." },
  discovered: { label: "Found by Domovoi", hint: "Observed on the host." },
  integration: { label: "From an integration", hint: "Reported by a connected service." },
  inferred: {
    label: "Inferred by Domovoi",
    hint: "Worked out from evidence. It is not yours until you accept it.",
  },
  imported: { label: "Imported", hint: "Brought in from a file or another system." },
};

/** The words for a source; an unknown one shows its raw key. */
export function sourceInfo(key: string): { hint: string; label: string } {
  return (Object.hasOwn(SOURCES, key) ? SOURCES[key] : undefined) ?? { label: key, hint: "" };
}

/** Each relation kind as the verb of a sentence: "plex runs on nas". */
export const RELATION_KINDS: Readonly<Record<string, string>> = {
  depends_on: "depends on",
  runs_on: "runs on",
  routes_to: "routes to",
  stores_on: "stores on",
  backs_up_to: "backs up to",
  managed_from: "is managed from",
};

/** The sentence form of a relation kind; an unknown one is its raw kind. */
export function relationPhrase(kind: string): string {
  return (Object.hasOwn(RELATION_KINDS, kind) ? RELATION_KINDS[kind] : undefined) ?? kind;
}

/** The label of each field of a resolution summary. */
export const SUMMARY_FIELDS: Readonly<Record<string, string>> = {
  outcome: "Outcome",
  verified: "Fix verified",
  subjects: "What it was about",
  contracts_executed: "What ran",
  hosts: "Hosts",
  problem: "Problem",
  cause: "Cause",
  fix: "Fix",
};

/** The summary fields the model drafts and the owner may confirm or edit. */
export const NARRATIVE_FIELDS: readonly string[] = ["problem", "cause", "fix"];

/** The label of a summary field; an unknown one shows its raw key. */
export function summaryFieldLabel(field: string): string {
  return (Object.hasOwn(SUMMARY_FIELDS, field) ? SUMMARY_FIELDS[field] : undefined) ?? field;
}

/** Who wrote each summary field. */
export const SUMMARY_SOURCES: Readonly<Record<string, { hint: string; label: string }>> = {
  code: { label: "Filled in by Domovoi", hint: "Taken from what the operation recorded." },
  model: {
    label: "Drafted by the model, not checked",
    hint: "The model wrote this from its own report. Confirm or edit it to make it yours.",
  },
  user: { label: "Written or confirmed by you", hint: "You wrote or confirmed this text." },
};

/** The words for a summary source; an unknown one shows its raw key. */
export function summarySourceInfo(key: string): { hint: string; label: string } {
  return (
    (Object.hasOwn(SUMMARY_SOURCES, key) ? SUMMARY_SOURCES[key] : undefined) ?? {
      label: key,
      hint: "",
    }
  );
}

/** Who recorded something, in words: `me` is the signed-in account's own `user:<id>`. */
export function actorLabel(actor: string, me: string | undefined): string {
  if (actor.startsWith("user:")) {
    return me !== undefined && (actor === me || actor === `user:${me}`) ? "You" : "Another account";
  }
  switch (actor) {
    case "system:knowledge":
      return "Domovoi (knowledge)";
    case "system:profile":
      return "Domovoi (host profile)";
    case "system:operations":
      return "Domovoi (operations)";
    default:
      return actor.startsWith("agent:") ? "A host's agent" : actor;
  }
}
