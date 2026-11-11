import { ICON_NAMES } from "./icons";
import { TONES, VOCABULARIES, badgeMeta, incidentBadgeMeta, verdictMeta } from "./tone";

describe("tone", () => {
  it.each(VOCABULARIES.flatMap((v) => v.values.map((value) => [v.name, value, v.meta] as const)))(
    "%s %s has a label, a tone, a hint and an icon",
    (_name, value, meta) => {
      const m = meta(value);
      expect(m.label).not.toBe("");
      expect(TONES).toContain(m.tone);
      expect(m.hint.length).toBeGreaterThan(10);
      expect(ICON_NAMES).toContain(m.icon);
    },
  );

  it.each(VOCABULARIES.map((v) => [v.name, v.meta] as const))(
    "an unknown %s falls back to neutral with the raw value as its label",
    (_name, meta) => {
      const m = meta("from_a_newer_hub");
      expect(m.label).toBe("from_a_newer_hub");
      expect(m.tone).toBe("neutral");
      expect(m.hint).not.toBe("");
    },
  );

  it("an inherited object key is not a known status", () => {
    expect(verdictMeta("toString").tone).toBe("neutral");
  });

  it("a resolved incident shows Resolved whatever its verdict", () => {
    for (const verdict of ["ACTION_NEEDED", "MONITOR", "EXPECTED", "FIXED", null]) {
      const m = incidentBadgeMeta({ status: "resolved", verdict, severity: "critical" });
      expect(m.label).toBe("Resolved");
      expect(m.tone).toBe("success");
    }
  });

  it("an unresolved incident shows its verdict, or its severity before triage", () => {
    expect(
      incidentBadgeMeta({ status: "open", verdict: "MONITOR", severity: "critical" }).label,
    ).toBe("Monitor");
    expect(incidentBadgeMeta({ status: "open", verdict: null, severity: "critical" }).label).toBe(
      "Critical",
    );
  });

  it("a chat card badge value is a verdict, a severity or resolved", () => {
    expect(badgeMeta("ACTION_NEEDED").tone).toBe("danger");
    expect(badgeMeta("warning").label).toBe("Warning");
    expect(badgeMeta("resolved").label).toBe("Resolved");
  });
});
