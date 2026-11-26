import { describe, expect, it } from "vitest";
import * as text from "../../design/containmentText";
import * as tables from "../../design/containmentVocab";

const vocab = { ...tables, ...text };
import { CONTAINMENT_CATALOGUE } from "../../design/containmentErrors";
import { phaseSteps } from "./phases";

/** Every string the containment vocabulary can put on screen. */
function everyWord(): string[] {
  const words: string[] = [];
  const walk = (value: unknown) => {
    if (typeof value === "string") {
      words.push(value);
    } else if (value !== null && typeof value === "object") {
      Object.values(value).forEach(walk);
    }
  };
  for (const exported of Object.values(vocab)) {
    if (typeof exported !== "function") {
      walk(exported);
    }
  }
  walk(CONTAINMENT_CATALOGUE);
  for (const flows of ["cut", "n/a", "may_persist", "other"]) {
    words.push(vocab.existingFlowsText(flows) ?? "");
  }
  for (const reason of ["key_not_pinned", "report_incomplete", "unknown"]) {
    words.push(vocab.unverifiedReason(reason));
  }
  for (const state of ["blocked_leak", "blocked_untested"]) {
    words.push(vocab.gateAttentionWhy(state));
  }
  for (const phase of Object.keys(vocab.BLOCK_PHASES)) {
    words.push(...phaseSteps(phase, "tomorrow").map((step) => step.label));
  }
  return words;
}

describe("containment words", () => {
  it("containment_words_never_say_contained_for_may_persist", () => {
    const flows = vocab.existingFlowsText("may_persist") ?? "";
    expect(flows).toMatch(/may keep working/);
    expect(flows.toLowerCase()).not.toContain("contained");
    expect(everyWord().join("\n").toLowerCase()).not.toContain("contained");
  });

  it("containment_words_never_claim_support", () => {
    const text = everyWord().join("\n").toLowerCase();
    expect(text).not.toMatch(/\bsupported\b|\bsupport\b|never absent|never be absent/);
  });

  it("every_block_phase_has_its_own_label_tone_and_hint", () => {
    for (const [phase, meta] of Object.entries(vocab.BLOCK_PHASES)) {
      expect(meta.label, phase).not.toBe("");
      expect(meta.hint, phase).not.toBe("");
    }
    expect(vocab.blockPhaseMeta("lost").tone).toBe("danger");
    expect(vocab.blockPhaseMeta("confirmed").tone).toBe("success");
  });

  it("phase_steps_follow_the_block_from_apply_to_where_it_ended", () => {
    const states = (phase: string) => phaseSteps(phase, null).map((step) => step.state);
    expect(states("proposed")).toEqual(["upcoming", "upcoming", "upcoming"]);
    expect(states("applying")).toEqual(["current", "upcoming", "upcoming"]);
    expect(states("applied")).toEqual(["done", "current", "upcoming"]);
    expect(states("confirmed")).toEqual(["done", "done", "current"]);
    expect(states("expired")).toEqual(["done", "upcoming", "done"]);
    expect(phaseSteps("confirmed", "Mon")[2]?.label).toBe("Blocked until Mon");
    expect(phaseSteps("lost", null)[2]?.label).toBe("Lost");
  });
});
