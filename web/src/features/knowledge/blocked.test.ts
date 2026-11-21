import { containmentBlock } from "./blocked";

describe("containmentBlock", () => {
  it("containment_block_follows_the_truth_table", () => {
    expect(containmentBlock({ coverageAvailable: false, states: [] })).toEqual({
      blocked: true,
      reasons: [
        "The helper's coverage cannot be verified yet.",
        "No management paths are recorded for this host.",
      ],
    });
    expect(
      containmentBlock({
        coverageAvailable: true,
        states: ["candidate", "candidate", "unconfirmed", "stale", "weird", "confirmed"],
      }).reasons,
    ).toEqual([
      "2 paths found by Domovoi are not accepted by you yet.",
      "1 path is not confirmed.",
      "1 path is stale.",
      "1 path is in a state this page does not know.",
    ]);
    expect(
      containmentBlock({ coverageAvailable: true, states: ["confirmed", "confirmed"] }),
    ).toEqual({
      blocked: false,
      reasons: [],
    });
    // Confirmed paths do not help while coverage cannot be checked.
    expect(containmentBlock({ coverageAvailable: false, states: ["confirmed"] })).toEqual({
      blocked: true,
      reasons: ["The helper's coverage cannot be verified yet."],
    });
    expect(containmentBlock({ coverageAvailable: true, states: [] }).blocked).toBe(true);
  });
});
