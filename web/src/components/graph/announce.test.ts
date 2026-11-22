import { describe, expect, it } from "vitest";
import { DEFAULT_GRAPH_LABEL, GRAPH_INSTRUCTIONS, graphLabel, liveLine } from "./announce";
import type { GraphNodeModel } from "./types";

const step: GraphNodeModel = {
  id: "s1",
  ordinal: 0,
  kind: "step",
  title: "Evidence",
  icon: "search",
  status: { label: "Succeeded", tone: "success", icon: "check", value: "succeeded" },
  summary: "plex is exited",
  members: [],
  retry: false,
};
const group: GraphNodeModel = {
  ...step,
  id: "group:s1",
  kind: "group",
  title: "3 technical steps",
  summary: "Model turn ×2, Evidence",
  members: ["s1", "s2", "s3"],
  status: { label: "Folded", tone: "neutral", icon: "dot", value: "" },
};
const ctx = (dependsOn: number, neededBy: number, selected = false) => ({
  dependsOn,
  neededBy,
  selected,
});

describe("graph announcements", () => {
  it("live_line_text_matches_the_spec", () => {
    expect(liveLine(step, ctx(2, 1))).toBe(
      "Evidence, Succeeded: plex is exited. Depends on 2 steps. Needed by 1 step.",
    );
    expect(liveLine(step, ctx(1, 2))).toBe(
      "Evidence, Succeeded: plex is exited. Depends on 1 step. Needed by 2 steps.",
    );
    expect(liveLine({ ...step, summary: null }, ctx(0, 0))).toBe("Evidence, Succeeded.");
    expect(liveLine(step, ctx(0, 3, true))).toBe(
      "Evidence, Succeeded: plex is exited. Needed by 3 steps. Selected.",
    );
    expect(liveLine(group, ctx(1, 1))).toBe(
      "3 technical steps folded: Model turn ×2, Evidence. Press Enter to show them.",
    );
    expect(liveLine({ ...group, title: "1 technical step", summary: null }, ctx(0, 0))).toBe(
      "1 technical step folded. Press Enter to show them.",
    );
  });

  it("graph_label_defaults_to_step_graph_and_instructions_are_the_spec_text", () => {
    expect(DEFAULT_GRAPH_LABEL).toBe("Step graph");
    expect(graphLabel()).toBe("Step graph");
    expect(graphLabel("  ")).toBe("Step graph");
    expect(graphLabel("Past steps")).toBe("Past steps");
    expect(GRAPH_INSTRUCTIONS).toBe(
      "Arrow keys move between connected steps. Page Down and Page Up move through steps in time order. Enter opens the step. Escape clears the selection. Plus, minus and zero zoom and fit.",
    );
  });
});
