const sources = import.meta.glob("../../**/*.{ts,tsx}", { query: "?raw", eager: true }) as Record<
  string,
  { default: string }
>;

/** Files that may call a mutation hook: its own file and the one control that uses it. */
const ALLOWED: Record<string, string[]> = {
  "useApproveStep(": ["operationControls.ts", "StepApproveControl.tsx"],
  "useApprove(": ["actions.ts", "ApproveControl.tsx"],
};

describe("approval gates", () => {
  it("approve_mutations_are_only_used_by_the_approve_controls", () => {
    const found: Record<string, string[]> = {};
    for (const [path, module] of Object.entries(sources)) {
      const name = path.split("/").pop() ?? path;
      if (/\.test\.tsx?$/.test(name) || path.includes("/test/")) {
        continue;
      }
      for (const call of Object.keys(ALLOWED)) {
        if (module.default.includes(call)) {
          (found[call] ??= []).push(name);
        }
      }
    }
    for (const [call, files] of Object.entries(ALLOWED)) {
      for (const file of found[call] ?? []) {
        expect(files, `${call} used in ${file}`).toContain(file);
      }
      expect(found[call]?.length ?? 0, `${call} is used by its control`).toBeGreaterThan(1);
    }
  });
});
