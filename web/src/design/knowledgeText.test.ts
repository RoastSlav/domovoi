import {
  NARRATIVE_FIELDS,
  PREDICATES,
  SAFETY_CRITICAL,
  actorLabel,
  needsPassword,
  predicateInfo,
  relationPhrase,
  sourceInfo,
  summaryFieldLabel,
  type KnowledgeAction,
} from "./knowledgeText";

const KEYS = [
  "role",
  "criticality",
  "owner_note",
  "depends_on",
  "management_path",
  "maintenance_window",
  "backup_location",
  "never_restart",
  "trusted_cidr",
  "host_platform",
];

describe("knowledge words", () => {
  it("predicate_table_matches_the_knowledge_store_rules", () => {
    expect(Object.keys(PREDICATES).sort()).toEqual([...KEYS].sort());
    expect([...SAFETY_CRITICAL].sort()).toEqual(["management_path", "trusted_cidr"]);
    expect(PREDICATES.host_platform?.assertable).toBe(false);
    expect(KEYS.filter((k) => PREDICATES[k]?.assertable)).toHaveLength(9);
    expect(PREDICATES.management_path?.subjects).toEqual(["host"]);
    expect(PREDICATES.trusted_cidr?.subjects).toEqual(["host"]);
    expect(PREDICATES.never_restart?.subjects).toEqual(["container", "unit"]);
    expect(PREDICATES.role?.subjects).toEqual(["host", "container", "unit", "entity"]);
    expect(PREDICATES.role?.many).toBe(false);
    expect(PREDICATES.owner_note?.many).toBe(true);
  });

  it("an unknown predicate shows its key and cannot be asserted", () => {
    const info = predicateInfo("from_a_newer_hub");
    expect(info.label).toBe("from_a_newer_hub");
    expect(info.assertable).toBe(false);
    expect(info.safety).toBe(false);
    expect(info.subjects).toHaveLength(4);
  });

  const actions: KnowledgeAction[] = ["assert", "accept", "verify", "retract", "confirm"];
  it.each(actions.flatMap((a) => KEYS.map((p) => [a, p] as const)))(
    "needs_password_covers_safety_predicates_and_every_confirm: %s %s",
    (action, predicate) => {
      const safety = predicate === "management_path" || predicate === "trusted_cidr";
      const expected = action === "confirm" ? true : action === "retract" ? false : safety;
      expect(needsPassword(action, predicate)).toBe(expected);
    },
  );

  it("actor_label_names_you_and_unknown_actors_stay_raw", () => {
    expect(actorLabel("user:abc", "user:abc")).toBe("You");
    expect(actorLabel("user:abc", "abc")).toBe("You");
    expect(actorLabel("user:abc", "user:other")).toBe("Another account");
    expect(actorLabel("user:abc", undefined)).toBe("Another account");
    expect(actorLabel("system:knowledge", undefined)).toBe("Domovoi (knowledge)");
    expect(actorLabel("system:profile", undefined)).toBe("Domovoi (host profile)");
    expect(actorLabel("system:operations", undefined)).toBe("Domovoi (operations)");
    expect(actorLabel("agent:x", undefined)).toBe("A host's agent");
    expect(actorLabel("something:else", undefined)).toBe("something:else");
  });

  it("sources, relation kinds and summary fields fall back to the raw key", () => {
    expect(sourceInfo("inferred").label).toBe("Inferred by Domovoi");
    expect(sourceInfo("new_source")).toEqual({ label: "new_source", hint: "" });
    expect(relationPhrase("runs_on")).toBe("runs on");
    expect(relationPhrase("is_new")).toBe("is_new");
    expect(summaryFieldLabel("contracts_executed")).toBe("What ran");
    expect(summaryFieldLabel("toString")).toBe("toString");
    expect(NARRATIVE_FIELDS).toEqual(["problem", "cause", "fix"]);
  });
});
