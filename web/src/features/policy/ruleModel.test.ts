import { describe, expect, it } from "vitest";
import type { PolicyRule } from "../../api/types";
import { diffCount, duplicateOf, fromDto, newRule, toDto, validateRule } from "./ruleModel";
import type { RuleForm } from "./ruleModel";

const full: PolicyRule = {
  id: "no-restart-at-night",
  enabled: true,
  shadow: false,
  locked: true,
  outcome: "deny",
  host: { kind: "host", host_id: "h-1" },
  capability: { kind: "contract", contract: "container.restart" },
  targets: { kind: "exact", names: ["api", "db"] },
  conditions: [
    { kind: "arg_constraint", arg: "count", op: "int_at_least", value: 2 },
    { kind: "time_window", start: "22:00", end: "06:00" },
    { kind: "requires_fact", fact: "disk.healthy", expected: false },
    { kind: "max_rate", max_per_hour: 4 },
    { kind: "max_ttl", max_ttl_secs: 600 },
  ],
  note: "Nights are quiet.",
};

const form = (patch: Partial<RuleForm> = {}): RuleForm => ({ ...newRule([]), ...patch });
const problems = (patch: Partial<RuleForm>, others: string[] = []) =>
  validateRule(form(patch), others);

describe("rule model", () => {
  it("an_unknown_outcome_is_kept_untouched_through_a_round_trip", () => {
    const rule = { ...full, outcome: "from_the_future" };
    const form = fromDto(rule);
    expect(form.unsupported).toEqual(rule);
    expect(toDto(form)).toEqual({ ok: true, rule });
  });

  it("an_unknown_capability_or_target_kind_is_kept_untouched", () => {
    const capability = { ...full, capability: { kind: "regex", pattern: "x" } } as never;
    const targets = { ...full, targets: { kind: "glob", pattern: "a*" } } as never;
    const host = { ...full, host: { kind: "group", name: "g" } } as never;
    for (const rule of [capability, targets, host] as PolicyRule[]) {
      expect(toDto(fromDto(rule))).toEqual({ ok: true, rule });
    }
    expect(duplicateOf(capability, []).unsupported).toMatchObject({
      id: "no-restart-at-night-copy",
    });
  });

  it("rule_to_dto_produces_the_documented_json", () => {
    const result = toDto(fromDto(full));
    expect(result).toEqual({ ok: true, rule: full });
    const any = toDto(newRule([]));
    expect(any).toEqual({
      ok: true,
      rule: {
        id: "new-rule",
        enabled: true,
        shadow: true,
        locked: false,
        outcome: "require_approval",
        host: { kind: "all" },
        capability: { kind: "any" },
        targets: { kind: "any" },
        conditions: [],
      },
    });
    const unlocked = toDto({ ...fromDto(full), outcome: "allow" });
    expect(unlocked.ok && unlocked.rule.locked).toBe(false);
  });

  it("rule_validation_follows_the_table", () => {
    expect(problems({ id: "" }).id).toBeDefined();
    expect(problems({ id: "-bad" }).id).toBeDefined();
    expect(problems({ id: "a".repeat(65) }).id).toBeDefined();
    expect(problems({ id: "ok.name:1_x-y" }).id).toBeUndefined();
    expect(problems({ id: "taken" }, ["taken"]).id).toBe("Another rule already has this name.");
    const contract = (capValue: string) => problems({ capKind: "contract", capValue }).capValue;
    expect(contract("container.restart")).toBeUndefined();
    expect(contract("skill.backup_db")).toBeUndefined();
    expect(contract("restart")).toBeDefined();
    expect(contract("Container.restart")).toBeDefined();
    expect(contract(`a.${"b".repeat(80)}`)).toBeDefined();
    const category = (capValue: string) => problems({ capKind: "category", capValue }).capValue;
    expect(category("container.lifecycle")).toBeUndefined();
    expect(category(`a.${"b".repeat(64)}`)).toBeDefined();
    expect(problems({ targetsKind: "exact", targetNames: " \n " }).targetNames).toBe(
      "Name at least one target, or choose Any target.",
    );
    expect(
      problems({ targetsKind: "exact", targetNames: "x".repeat(129) }).targetNames,
    ).toBeDefined();
    expect(problems({ targetsKind: "exact", targetNames: "a\u0001b" }).targetNames).toBeDefined();
    expect(problems({ note: "n".repeat(501) }).note).toBe("At most 500 characters.");
    const cond = (c: object) => {
      const f = form({ conditions: [{ key: "k", ...c } as never] });
      return validateRule(f).conditions.k;
    };
    expect(
      cond({ kind: "arg_constraint", arg: "Count", op: "int_at_least", value: "1" }),
    ).toBeDefined();
    expect(cond({ kind: "arg_constraint", arg: "count", op: "int_at_least", value: "1.5" })).toBe(
      "Enter a whole number.",
    );
    expect(
      cond({ kind: "arg_constraint", arg: "count", op: "int_at_least", value: "-3" }),
    ).toBeUndefined();
    expect(cond({ kind: "time_window", start: "22:00", end: "22:00" })).toBe(
      "Start and end must differ.",
    );
    expect(cond({ kind: "time_window", start: "25:00", end: "22:00" })).toBeDefined();
    expect(cond({ kind: "requires_fact", fact: "Bad", expected: "true" })).toBeDefined();
    expect(cond({ kind: "max_rate", max: "-1" })).toBe("Enter a whole number.");
    expect(cond({ kind: "max_ttl", seconds: "-1" })).toBe("Enter a whole number.");
    expect(cond({ kind: "max_ttl", seconds: "30" })).toBeUndefined();
    expect(toDto(form({ id: "" })).ok).toBe(false);
  });

  it("new_rules_start_as_shadow_rules_and_duplicates_never_lock", () => {
    const fresh = newRule(["new-rule", "new-rule-2"]);
    expect(fresh).toMatchObject({
      id: "new-rule-3",
      enabled: true,
      shadow: true,
      outcome: "require_approval",
    });
    expect(fresh.hostId).toBe("");
    expect(fresh.conditions).toEqual([]);
    const copy = duplicateOf(full, ["no-restart-at-night", "no-restart-at-night-copy"]);
    expect(copy.id).toBe("no-restart-at-night-copy-2");
    expect(copy.locked).toBe(false);
    expect(copy.shadow).toBe(true);
    expect(duplicateOf(full, []).id).toBe("no-restart-at-night-copy");
  });

  it("target_names_are_trimmed_deduplicated_and_never_empty", () => {
    const result = toDto(form({ targetsKind: "exact", targetNames: " api \n\ndb\r\napi\n  " }));
    expect(result.ok && result.rule.targets).toEqual({ kind: "exact", names: ["api", "db"] });
    const empty = toDto(form({ targetsKind: "exact", targetNames: "\n  \n" }));
    expect(empty.ok).toBe(false);
  });

  it("diff_count_counts_added_removed_and_changed_rules", () => {
    const a: PolicyRule = { ...full, id: "a" };
    const b: PolicyRule = { ...full, id: "b" };
    expect(diffCount([a, b], [a, b])).toBe(0);
    expect(diffCount([a], [a, b])).toBe(1);
    expect(diffCount([a, b], [a])).toBe(1);
    expect(diffCount([a, b], [a, { ...b, enabled: false }])).toBe(1);
    expect(diffCount([a, b], [{ ...b, note: "x" }])).toBe(2);
    const noConditions: PolicyRule = { ...a };
    delete noConditions.conditions;
    delete noConditions.note;
    expect(diffCount([noConditions], [{ ...a, conditions: [], note: "" }])).toBe(0);
  });

  it("an_unknown_condition_is_kept_untouched_through_a_round_trip", () => {
    const future = { kind: "moon_phase", phase: "full", extra: [1, 2] };
    const rule = { ...full, conditions: [future] } as unknown as PolicyRule;
    const result = toDto(fromDto(rule));
    expect(result.ok && result.rule.conditions).toEqual([future]);
  });
});
