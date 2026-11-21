import { describe, expect, it } from "vitest";
import { DECISION_EXCEPTION, DECISION_LIMITS, DECISION_LOCKED } from "../../test/policyFixtures";
import { decisionOf, gapsOf, parseDuration } from "./decisionModel";

const doc = {
  outcome: { kind: "deny" },
  reason: "locked_deny",
  matched: ["a", "b"],
  winners: ["a"],
  locked: ["a"],
  overridden: [{ broader: "b", by: "a" }],
  would_have_been: { outcome: { kind: "allow" }, reason: "default" },
  constraints: { max_per_hour: 3, max_ttl_secs: 60, targets: ["api"], conditions: [] },
  hard_limits_applied: ["target_not_pinned"],
  facts_used: [{ fact: "disk.healthy", status: "stale" }],
  typed_confirmation: "typed",
  warnings: [
    { kind: "dangerous_exception", allow: "x", deny: "y" },
    { kind: "safety_set_unverified" },
    { kind: "management_paths_unprotected", paths: ["/etc"] },
    { kind: "from_the_future" },
  ],
  ceiling_hash: "abc",
};

describe("decision model", () => {
  it("decision_of_reads_a_decision_an_error_and_null", () => {
    expect(decisionOf(null)).toBeNull();
    expect(decisionOf(undefined)).toBeNull();
    expect(decisionOf({ error: "empty_target_set" })).toEqual({
      kind: "error",
      code: "empty_target_set",
    });
    const view = decisionOf(doc);
    expect(view?.kind).toBe("decision");
    if (view?.kind !== "decision") return;
    const d = view.doc;
    expect(d).toMatchObject({
      outcome: "deny",
      unavailableReason: null,
      reason: "locked_deny",
      matched: ["a", "b"],
      winners: ["a"],
      locked: ["a"],
      overridden: [{ broader: "b", by: "a" }],
      limits: ["target_not_pinned"],
      facts: [{ fact: "disk.healthy", status: "stale" }],
      typedConfirmation: true,
      ceilingHash: "abc",
      constraints: { maxPerHour: 3, maxTtlSeconds: 60, targets: ["api"], conditions: [] },
    });
    expect(d.wouldHaveBeen?.outcome).toBe("allow");
    expect(d.warnings.map((w) => w.kind)).toEqual([
      "dangerous_exception",
      "safety_set_unverified",
      "management_paths_unprotected",
      "unknown",
    ]);
    const unavailable = decisionOf({
      outcome: { kind: "unavailable", reason: "safety_unknown" },
      typed_confirmation: "none",
    });
    expect(unavailable).toMatchObject({
      doc: {
        outcome: "unavailable",
        unavailableReason: "safety_unknown",
        typedConfirmation: false,
      },
    });
    expect(() => decisionOf(42)).not.toThrow();
    expect(() =>
      decisionOf({ outcome: 5, constraints: 7, warnings: "x", facts_used: [null] }),
    ).not.toThrow();
    expect(
      gapsOf([{ input: "ceiling", reason: "host_not_connected" }, 3, { reason: "x" }]),
    ).toEqual([{ input: "ceiling", reason: "host_not_connected" }]);
    expect(gapsOf(undefined)).toEqual([]);
  });

  it("decision_of_reads_both_condition_shapes_and_keeps_unknown_ones", () => {
    const view = decisionOf({
      outcome: { kind: "allow" },
      constraints: {
        conditions: [
          { kind: "time_window", start: { hour: 6, minute: 5 }, end: "22:00" },
          { kind: "max_ttl", max_ttl: "PT1H30M" },
          { kind: "max_ttl", max_ttl_secs: 45 },
          { kind: "max_ttl", max_ttl: "soon" },
          { kind: "arg_constraint", arg: "n", op: "int_at_most", value: 4 },
          { kind: "requires_fact", fact: "f", expected: true },
          { kind: "max_rate", max_per_hour: 9 },
          { kind: "lunar", x: 1 },
        ],
      },
    });
    const list = view?.kind === "decision" ? view.doc.constraints.conditions : [];
    expect(list).toEqual([
      { kind: "time_window", start: "06:05", end: "22:00" },
      { kind: "max_ttl", seconds: 5400, raw: "PT1H30M" },
      { kind: "max_ttl", seconds: 45, raw: null },
      { kind: "max_ttl", seconds: null, raw: "soon" },
      { kind: "arg_constraint", arg: "n", op: "int_at_most", value: 4 },
      { kind: "requires_fact", fact: "f", expected: true },
      { kind: "max_rate", maxPerHour: 9 },
      { kind: "unknown", raw: '{"kind":"lunar","x":1}' },
    ]);
  });

  it("parse_duration_understands_iso_and_friendly_text_and_nothing_else", () => {
    expect(parseDuration("PT10M")).toBe(600);
    expect(parseDuration("PT1H30M")).toBe(5400);
    expect(parseDuration("PT45S")).toBe(45);
    expect(parseDuration("10m")).toBe(600);
    expect(parseDuration("1h 30m")).toBe(5400);
    expect(parseDuration("45s")).toBe(45);
    expect(parseDuration("")).toBeNull();
    expect(parseDuration("PT")).toBeNull();
    expect(parseDuration("soon")).toBeNull();
    expect(parseDuration("10")).toBeNull();
    expect(parseDuration("-5m")).toBeNull();
  });

  it("decision_of_marks_a_decision_incomplete_when_it_drops_something", () => {
    const view = decisionOf({
      outcome: { kind: "allow" },
      matched: ["a", 7],
      facts_used: ["not an object"],
      from_a_newer_hub: true,
      warnings: [{ kind: "from_the_future" }],
    });
    if (view?.kind !== "decision") throw new Error("expected a decision");
    expect(view.doc.incomplete).toBe(true);
    expect(view.doc.matched).toEqual(["a"]);
    expect(view.doc.unreadable).toEqual(
      expect.arrayContaining([
        "a matched rule entry",
        "a fact entry",
        'the field "from_a_newer_hub"',
        "a warning this page does not know",
      ]),
    );
  });

  it("decision_of_is_complete_for_the_fixtures", () => {
    for (const fixture of [DECISION_LOCKED, DECISION_EXCEPTION, DECISION_LIMITS]) {
      for (const side of [fixture.enforced, fixture.shadow]) {
        const view = decisionOf(side);
        if (view?.kind !== "decision") continue;
        expect(view.doc.unreadable).toEqual([]);
        expect(view.doc.incomplete).toBe(false);
      }
    }
  });
});
