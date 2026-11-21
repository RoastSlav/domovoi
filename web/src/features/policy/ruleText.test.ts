import { describe, expect, it } from "vitest";
import type { PolicyRule } from "../../api/types";
import { decisionOf } from "./decisionModel";
import { actorText, conditionText, conditionViewText, scopeText } from "./ruleText";

const base: PolicyRule = {
  id: "r1",
  enabled: true,
  shadow: false,
  locked: false,
  outcome: "deny",
  host: { kind: "all" },
  capability: { kind: "any" },
  targets: { kind: "any" },
};

describe("rule texts", () => {
  it("scope_text_names_host_capability_and_targets", () => {
    const hostId = "0190aaaa-bbbb-cccc-dddd-eeeeffff0000";
    const rule: PolicyRule = {
      ...base,
      host: { kind: "host", host_id: hostId },
      capability: { kind: "contract", contract: "container.restart" },
      targets: { kind: "exact", names: ["api", "db"] },
    };
    const text = scopeText(rule, new Map([[hostId, "web-1"]]));
    expect(text).toContain("web-1");
    expect(text).toContain("container.restart");
    expect(text).toContain("api, db");
    expect(scopeText(rule, { [hostId]: "web-2" })).toContain("web-2");
    expect(scopeText(rule, {})).toContain("Unknown host (0190aaaa)");
    expect(scopeText(base, {})).toBe("On every host: any action on any target.");
    expect(
      scopeText({ ...base, capability: { kind: "effect", effect: "destructive" } }, {}),
    ).toContain("any destructive action");
    expect(
      scopeText({ ...base, capability: { kind: "category", category: "container.lifecycle" } }, {}),
    ).toContain("container.lifecycle actions");
  });

  it("condition_text_names_the_zone_it_was_given", () => {
    const window = { kind: "time_window", start: "22:00", end: "06:00" } as const;
    expect(conditionText(window, "UTC")).toContain("between 22:00 and 06:00 UTC");
    expect(conditionText(window, "Europe/Berlin")).toContain(
      "between 22:00 and 06:00 Europe/Berlin",
    );
    expect(conditionText(window, undefined)).toContain(
      "between 22:00 and 06:00 in the hub's time zone",
    );
    expect(conditionText(window, "")).toContain("in the hub's time zone");
  });

  it("condition_and_duration_texts_are_plain", () => {
    const ttl = (secs: number) => conditionText({ kind: "max_ttl", max_ttl_secs: secs }, "UTC");
    expect(ttl(45)).toBe("Lasts at most 45 s");
    expect(ttl(600)).toBe("Lasts at most 10 min");
    expect(ttl(5400)).toBe("Lasts at most 1 h 30 min");
    expect(conditionText({ kind: "max_rate", max_per_hour: 3 }, "UTC")).toBe("At most 3 per hour");
    expect(
      conditionText({ kind: "arg_constraint", arg: "count", op: "int_at_most", value: 5 }, "UTC"),
    ).toBe("Argument count is at most 5");
    expect(
      conditionText({ kind: "requires_fact", fact: "disk.healthy", expected: true }, "UTC"),
    ).toContain("disk.healthy");
    const doc = decisionOf({
      constraints: {
        conditions: [
          { kind: "max_ttl", max_ttl: "PT10M" },
          { kind: "time_window", start: { hour: 7, minute: 5 }, end: { hour: 9, minute: 0 } },
          { kind: "max_rate", max_per_hour: 2 },
        ],
      },
    });
    const views = doc?.kind === "decision" ? doc.doc.constraints.conditions : [];
    expect(views.map((v) => conditionViewText(v, "UTC"))).toEqual([
      "Lasts at most 10 min",
      "Only between 07:05 and 09:00 UTC",
      "At most 2 per hour",
    ]);
  });

  it("actor_text_says_a_person_or_domovoi", () => {
    expect(actorText("user:ana")).toBe("a person");
    expect(actorText("system:policy")).toBe("Domovoi");
    expect(actorText("cli")).toBe("cli");
  });
});
