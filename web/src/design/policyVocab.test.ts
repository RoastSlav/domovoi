import { ICON_NAMES } from "./icons";
import { POLICY_VOCABULARIES } from "./policyMeta";
import {
  comparisonMeta,
  decisionErrorMeta,
  decisionReasonMeta,
  effectClassMeta,
  factStatusMeta,
  findingKindMeta,
  gapInputMeta,
  gapReasonMeta,
  hardLimitMeta,
  lockChangeMeta,
  policyOutcomeMeta,
  ruleMarkMeta,
  shadowReasonMeta,
  shadowStatusMeta,
} from "./tone";

const COUNTS: Record<string, number> = {
  "Policy outcome": 5,
  "Rule mark": 5,
  "Decision reason": 5,
  "Hard limit": 10,
  "Policy finding": 4,
  "Lock change": 4,
  "Shadow status": 3,
  "Replay comparison": 3,
  "Fact status": 4,
  "Effect class": 4,
  "Replay gap input": 8,
  "Replay gap reason": 3,
  "Shadow reason": 6,
  "Decision error": 14,
};

type Shown = { icon: string; label: string; tone: string };

function triple(meta: Shown): [string, string, string] {
  return [meta.label, meta.tone, meta.icon];
}

test("every_policy_value_the_contract_names_has_words", () => {
  expect(POLICY_VOCABULARIES.map((entry) => entry.name).sort()).toEqual(Object.keys(COUNTS).sort());
  for (const { meta, name, values } of POLICY_VOCABULARIES) {
    expect(values, name).toHaveLength(COUNTS[name] ?? -1);
    for (const value of values) {
      const shown = meta(value);
      expect(shown.label, `${name} ${value}`).not.toBe(value);
      expect(shown.hint.length, `${name} ${value}`).toBeGreaterThan(10);
      expect(shown.hint, `${name} ${value}`).not.toContain("!");
      expect(shown.hint, `${name} ${value}`).toMatch(/^[A-Z].*\.$/);
      expect(ICON_NAMES).toContain(shown.icon);
    }
  }
});

test("labels_tones_and_icons_match_the_fixed_tables", () => {
  expect(triple(policyOutcomeMeta("allow"))).toEqual([
    "Allow automatically",
    "success",
    "check-circle",
  ]);
  expect(triple(policyOutcomeMeta("require_approval"))).toEqual([
    "Ask for approval",
    "pending",
    "clock",
  ]);
  expect(triple(policyOutcomeMeta("deny"))).toEqual(["Deny", "neutral", "ban"]);
  expect(triple(policyOutcomeMeta("unavailable"))).toEqual(["Cannot decide", "warning", "alert"]);
  expect(triple(policyOutcomeMeta("error"))).toEqual(["Could not evaluate", "warning", "alert"]);
  expect(triple(ruleMarkMeta("locked_deny"))).toEqual(["Locked Deny", "danger", "lock"]);
  expect(triple(ruleMarkMeta("shadow"))).toEqual(["Shadow", "info", "eye"]);
  expect(triple(ruleMarkMeta("disabled"))).toEqual(["Disabled", "neutral", "pause"]);
  expect(triple(ruleMarkMeta("dangerous_exception"))).toEqual([
    "Allow overrides a broader Deny",
    "warning",
    "alert",
  ]);
  expect(triple(ruleMarkMeta("not_enforced"))).toEqual(["Not enforced", "info", "info"]);
  expect(decisionReasonMeta("incomparable_fallback").label).toBe(
    "Conflicting rules, safer outcome",
  );
  expect(decisionReasonMeta("incomparable_fallback").hint).toContain("Deny, then Ask for approval");
  expect(triple(hardLimitMeta("containment_target_excluded"))).toEqual([
    "The target is excluded from containment",
    "danger",
    "ban",
  ]);
  expect(triple(hardLimitMeta("agent_runs_as_root"))).toEqual([
    "The agent runs as root",
    "warning",
    "alert",
  ]);
  expect(triple(findingKindMeta("ineffective_under_lock"))).toEqual([
    "Overridden by a locked rule",
    "danger",
    "lock",
  ]);
  for (const change of ["created", "edited", "unlocked", "removed"]) {
    expect(lockChangeMeta(change).tone).toBe("danger");
    expect(lockChangeMeta(change).icon).toBe("lock");
  }
  expect(triple(shadowStatusMeta("skipped"))).toEqual(["Skipped", "neutral", "skip"]);
  expect(triple(comparisonMeta("not_replayable"))).toEqual(["Cannot replay", "neutral", "help"]);
  expect(triple(factStatusMeta("stale"))).toEqual(["Stale", "warning", "clock"]);
  expect(triple(effectClassMeta("destructive"))).toEqual(["Destructive", "danger", "trash"]);
  expect(triple(effectClassMeta("contain"))).toEqual(["Contain", "warning", "shield"]);
  expect(gapInputMeta("runs_as_root").label).toBe("Agent privilege");
  expect(triple(gapReasonMeta("host_not_connected"))).toEqual([
    "Host not connected",
    "warning",
    "alert",
  ]);
  expect(shadowReasonMeta("package_contract").label).toBe("Package action, not evaluated");
  expect(decisionErrorMeta("too_many_facts").tone).toBe("warning");
});

test("unknown_policy_values_are_neutral_with_the_raw_value", () => {
  for (const { meta, name } of POLICY_VOCABULARIES) {
    const shown = meta("from_the_future");
    expect(shown.label, name).toBe("from_the_future");
    expect(shown.tone, name).toBe("neutral");
  }
});

test("locked_deny_reads_differently_from_deny", () => {
  const locked = ruleMarkMeta("locked_deny");
  const deny = policyOutcomeMeta("deny");
  expect(locked.label).not.toBe(deny.label);
  expect(locked.icon).toBe("lock");
  expect(deny.icon).not.toBe("lock");
  expect(locked.tone).not.toBe(deny.tone);
});
