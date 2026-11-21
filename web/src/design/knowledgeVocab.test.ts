import { COVERAGE_STATES, KNOWLEDGE_STATUSES, PATH_STATES } from "./knowledgeVocab";
import { ICON_NAMES } from "./icons";
import { TONES, coverageMeta, knowledgeStatusMeta, pathStateMeta } from "./tone";

const TABLES = [
  [
    "knowledge status",
    KNOWLEDGE_STATUSES,
    knowledgeStatusMeta,
    ["current", "stale", "disputed", "superseded", "retracted"],
  ],
  ["path state", PATH_STATES, pathStateMeta, ["candidate", "unconfirmed", "confirmed", "stale"]],
] as const;

describe("knowledge vocabularies", () => {
  it.each(TABLES)(
    "knowledge_vocabularies_have_label_tone_hint_and_icon: %s",
    (_n, table, meta, keys) => {
      expect(Object.keys(table).sort()).toEqual([...keys].sort());
      for (const key of keys) {
        const m = meta(key);
        expect(m.label).not.toBe("");
        expect(TONES).toContain(m.tone);
        expect(m.hint.length).toBeGreaterThan(10);
        expect(ICON_NAMES).toContain(m.icon);
      }
    },
  );

  it("an unknown status or path state is a neutral pill with the raw value", () => {
    for (const m of [knowledgeStatusMeta("from_a_newer_hub"), pathStateMeta("from_a_newer_hub")]) {
      expect(m.label).toBe("from_a_newer_hub");
      expect(m.tone).toBe("neutral");
    }
  });

  it("coverage_is_never_called_covered", () => {
    expect(Object.keys(COVERAGE_STATES).sort()).toEqual(["checkable", "unavailable"]);
    const words = [KNOWLEDGE_STATUSES, PATH_STATES, COVERAGE_STATES].flatMap((table) =>
      Object.values(table).flatMap((m) => [m.label, m.hint]),
    );
    for (const text of words) {
      expect(text).not.toMatch(/covered|protected/i);
    }
    expect(coverageMeta(false).tone).toBe("warning");
    expect(coverageMeta(false).label).toBe("Cannot be verified yet");
    expect(coverageMeta(true).label).toBe("Checked when you confirm");
  });
});
