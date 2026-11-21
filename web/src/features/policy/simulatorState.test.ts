import { describe, expect, it } from "vitest";
import { issuesByRule } from "./ruleIssues";
import { parseSimulatorParams, withSimulatorParams } from "./simulatorState";

describe("simulator state", () => {
  it("simulator_state_round_trips_the_url_and_falls_back_on_bad_values", () => {
    const state = parseSimulatorParams(
      new URLSearchParams("rules=version-7&selection=shadow&inputs=current&execution=e1&item=e2"),
    );
    expect(state).toEqual({
      rules: { source: "version", number: 7 },
      selection: "shadow",
      inputs: "current",
      execution: "e1",
      item: "e2",
    });
    const bad = parseSimulatorParams(new URLSearchParams("rules=nope&selection=x&inputs=y"), {
      rules: { source: "version", number: 2 },
    });
    expect(bad).toEqual({
      rules: { source: "version", number: 2 },
      selection: "both",
      inputs: "recorded",
      execution: null,
      item: null,
    });
    expect(parseSimulatorParams(new URLSearchParams()).rules).toEqual({ source: "draft" });
    const original = new URLSearchParams("tab=x&item=old");
    const next = withSimulatorParams(original, {
      rules: { source: "version", number: 3 },
      selection: "enforced",
      item: null,
    });
    expect(next.toString()).toBe("tab=x&rules=version-3&selection=enforced");
    expect(original.toString()).toBe("tab=x&item=old");
    expect(
      parseSimulatorParams(
        withSimulatorParams(next, { rules: { source: "draft" }, inputs: "current" }),
      ),
    ).toMatchObject({
      rules: { source: "draft" },
      selection: "enforced",
      inputs: "current",
    });
  });

  it("issues_by_rule_maps_rules_paths_to_rows_and_keeps_the_rest", () => {
    const { byRule, rest } = issuesByRule([
      { path: "rules.a", message: "one" },
      { path: "rules.a", message: "two", severity: "error" },
      { path: "rules.b.c", message: "dotted id" },
      { path: "", message: "general" },
      { path: "draft", message: "other" },
    ]);
    expect(byRule.get("a")).toEqual(["one", "two"]);
    expect(byRule.get("b.c")).toEqual(["dotted id"]);
    expect(rest).toEqual(["general", "other"]);
  });
});
