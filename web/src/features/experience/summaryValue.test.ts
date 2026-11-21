import { summaryValue } from "./summaryValue";

const hosts = new Map([["h1", "nas"]]);

describe("summaryValue", () => {
  it("summary_value_handles_every_field_and_odd_shapes", () => {
    expect(summaryValue("outcome", "succeeded", hosts)).toBe("Succeeded");
    expect(summaryValue("outcome", "from_a_newer_hub", hosts)).toBe("from_a_newer_hub");
    expect(summaryValue("verified", true, hosts)).toBe("Yes");
    expect(summaryValue("verified", false, hosts)).toBe("No");
    expect(
      summaryValue("subjects", [{ host: "h1", check: "container.down", target: "plex" }], hosts),
    ).toBe("container.down on plex");
    expect(summaryValue("subjects", [{ host: "h1", check: "disk.full" }], hosts)).toBe(
      "disk.full on nas",
    );
    expect(
      summaryValue(
        "contracts_executed",
        [{ contract: "skill.restart_container", host: "h1", target: "plex", outcome: "succeeded" }],
        hosts,
      ),
    ).toBe("skill.restart_container on plex: succeeded");
    expect(summaryValue("hosts", ["h1", "gone"], hosts)).toBe("nas; an unknown host");
    for (const field of ["problem", "cause", "fix"]) {
      expect(summaryValue(field, "plex was stopped", hosts)).toBe("plex was stopped");
    }
    expect(summaryValue("problem", "<b>x</b>", hosts)).toBe("<b>x</b>");
  });

  it("an unexpected shape is compact JSON and never throws", () => {
    expect(summaryValue("verified", "maybe", hosts)).toBe('"maybe"');
    expect(summaryValue("subjects", [{ nope: 1 }], hosts)).toBe('[{"nope":1}]');
    expect(summaryValue("hosts", "h1", hosts)).toBe('"h1"');
    expect(summaryValue("fix", { a: 1 }, hosts)).toBe('{"a":1}');
    expect(summaryValue("fix", null, hosts)).toBe("null");
    expect(summaryValue("fix", "x".repeat(10), hosts)).toHaveLength(10);
    expect(
      summaryValue(
        "subjects",
        [{ check: "x".repeat(1000), nope: 1 }].map((s) => ({ ...s, check: 5 })),
        hosts,
      ).length,
    ).toBeLessThanOrEqual(401);
    expect(() => summaryValue("fix", 10n, hosts)).not.toThrow();
  });
});
