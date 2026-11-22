import { describe, expect, it } from "vitest";
import { edgeStyle } from "./edgeStyle";

describe("edge style", () => {
  it("edge_style_gives_every_known_kind_a_distinct_pattern_and_unknown_kinds_a_solid_one", () => {
    expect(edgeStyle("depends_on")).toEqual({ dash: undefined, weight: 1.5 });
    expect(edgeStyle("caused_by")).toEqual({ dash: "6 4", weight: 1.5 });
    expect(edgeStyle("verifies")).toEqual({ dash: "2 3", weight: 2 });
    expect(edgeStyle("retry_of")).toEqual({ dash: "8 3 2 3", weight: 2 });
    expect(edgeStyle("answers")).toEqual({ dash: "12 4", weight: 1.5 });
    expect(edgeStyle("links")).toEqual({ dash: undefined, weight: 1 });
    expect(edgeStyle("toString")).toEqual({ dash: undefined, weight: 1 });
    const known = ["depends_on", "caused_by", "verifies", "retry_of", "answers"];
    const pairs = known.map((k) => JSON.stringify(edgeStyle(k)));
    expect(new Set(pairs).size).toBe(known.length);
  });
});
