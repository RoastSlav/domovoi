const sources = import.meta.glob("../../**/*.{ts,tsx}", { query: "?raw", eager: true }) as Record<
  string,
  { default: string }
>;

/**
 * Files that may call a mutation hook: its own file and the one control that uses it. Actions
 * can no longer be approved or denied, so no `useApprove(` or `useDeny(` may exist at all.
 */
/** Hooks of the removed action approval: they must not come back. */
const REMOVED = [
  "useApprove(",
  "useDeny(",
  "/api/v1/actions/{id}/approve",
  "/api/v1/actions/{id}/deny",
];

const ALLOWED: Record<string, string[]> = {
  "useApproveStep(": [
    "operationControls.ts",
    "StepApproveControl.tsx",
    "UnverifiedApproveControl.tsx",
  ],
};

describe("approval gates", () => {
  it("approve_mutations_are_only_used_by_the_approve_controls", () => {
    const found: Record<string, string[]> = {};
    for (const [path, module] of Object.entries(sources)) {
      const name = path.split("/").pop() ?? path;
      if (/\.test\.tsx?$/.test(name) || path.includes("/test/")) {
        continue;
      }
      for (const call of REMOVED) {
        if (module.default.includes(call)) {
          (found["removed"] ??= []).push(`${name}: ${call}`);
        }
      }
      for (const call of Object.keys(ALLOWED)) {
        if (module.default.includes(call)) {
          (found[call] ??= []).push(name);
        }
      }
    }
    expect(found["removed"] ?? [], "action approve and deny are gone").toEqual([]);
    for (const [call, files] of Object.entries(ALLOWED)) {
      for (const file of found[call] ?? []) {
        expect(files, `${call} used in ${file}`).toContain(file);
      }
      expect(found[call]?.length ?? 0, `${call} is used by its control`).toBeGreaterThan(1);
    }
  });
});
