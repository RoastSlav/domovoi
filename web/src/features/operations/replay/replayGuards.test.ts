// @vitest-environment node
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const OPERATIONS = join(process.cwd(), "src", "features", "operations");
const SOURCES = [
  ...readdirSync(join(OPERATIONS, "replay"))
    .filter((name) => /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name))
    .map((name) => join(OPERATIONS, "replay", name)),
  join(OPERATIONS, "GraphView.tsx"),
  join(process.cwd(), "src", "components", "Scrubber.tsx"),
  join(process.cwd(), "src", "api", "hooks", "operationHistory.ts"),
];
const text = (path: string): string => readFileSync(path, "utf8");

describe("the replay code", () => {
  it("replay_code_has_no_decision_controls", () => {
    expect(SOURCES.length).toBeGreaterThan(8);
    const forbidden: [string, RegExp][] = [
      ["a controls hook import", /operationControls/],
      [
        "an approve, deny, cancel, pause, resume or answer hook",
        /\buse(Approve|Deny|Cancel|Pause|Resume|Answer)\w*/,
      ],
      ["a mutation", /\.mutate(Async)?\(|useMutation|useActionFeedback/],
      ["a decision button", /["'>]\s*(Approve|Deny|Resume|Cancel operation|Answer)\b/],
      ["an interval", /\bsetInterval\b/],
      ["injected markup", /dangerouslySetInnerHTML/],
      ["a read of the clock inside the pure fold", /Date\.now\(\)/],
    ];
    for (const path of SOURCES) {
      const source = text(path);
      for (const [what, pattern] of forbidden) {
        // The replay's own words mention none of these; a match means a decision path crept in.
        expect(source, `${path} has ${what}`).not.toMatch(pattern);
      }
    }
  });

  it("replay_code_reads_no_profile_and_stores_nothing", () => {
    for (const path of SOURCES.filter((p) => p.includes("replay"))) {
      const source = text(path);
      expect(source, path).not.toMatch(
        /useProfile\b|visibilityOf\b|localStorage|sessionStorage|fetch\(/,
      );
    }
  });
});
