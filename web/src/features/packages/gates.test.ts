const modules = import.meta.glob("../../**/*.{ts,tsx}", {
  query: "?raw",
  eager: true,
  import: "default",
}) as Record<string, string>;

/** A glob key (relative to this folder) as a path under `src/`. */
function fromSrc(key: string): string {
  if (key.startsWith("./")) {
    return `features/packages/${key.slice(2)}`;
  }
  if (key.startsWith("../../")) {
    return key.slice(6);
  }
  return `features/${key.slice(3)}`;
}

/** Production sources: every file that is not a test, a fixture folder or a test helper. */
const sources = Object.entries(modules)
  .map(([path, text]) => ({ path: fromSrc(path), text }))
  .filter(
    ({ path }) =>
      !/\.test\.tsx?$/.test(path) && !path.startsWith("test/") && !path.endsWith("jobFixtures.ts"),
  );

const named = (...files: string[]) =>
  sources.filter(({ path }) => files.some((file) => path.endsWith(`/${file}`) || path === file));

function filesWith(text: string): string[] {
  return sources
    .filter((source) => source.text.includes(text))
    .map(({ path }) => path.split("/").pop() ?? path)
    .sort();
}

describe("package gates", () => {
  it("job_mutations_have_exactly_two_call_sites", () => {
    expect(filesWith("useCreateInstallJob(")).toEqual(["AuthorizeDialog.tsx", "packageJobs.ts"]);
    expect(filesWith("useRetryTarget(")).toEqual(["RetryDialog.tsx", "packageJobs.ts"]);
  });

  it("visibility_never_gates_package_authority", () => {
    const authority = named(
      "AuthorizeDialog.tsx",
      "RetryDialog.tsx",
      "wizardState.ts",
      "jobModel.ts",
      "api/hooks/packages.ts",
      "api/hooks/packageJobs.ts",
      "api/hooks/hostPackages.ts",
      "api/hooks/ceiling.ts",
    );
    expect(authority.length).toBeGreaterThanOrEqual(8);
    const words = [
      "useVisibility",
      "useProfile",
      "Visible",
      "presentationFor",
      "DetailsScope",
      "visibilityAttrs",
    ];
    for (const { path, text } of authority) {
      for (const word of words) {
        expect(text, `${path} reads the display profile (${word})`).not.toContain(word);
      }
    }
  });

  it("capability_center_has_no_authority_controls", () => {
    const writes = ["api.POST", "api.PUT", "api.DELETE", "useMutation"];
    const center = sources.filter(({ path }) => path.startsWith("features/capabilities/"));
    for (const { path, text } of center) {
      for (const word of [...writes, ".mutate("]) {
        expect(text, `${path} writes through ${word}`).not.toContain(word);
      }
    }
    const packages = sources.filter(({ path }) => path.startsWith("features/packages/"));
    expect(packages.length).toBeGreaterThan(5);
    for (const { path, text } of [...center, ...packages]) {
      for (const word of [
        ...(path.startsWith("features/packages/") ? writes : []),
        "trusted_keys",
        "allow_install",
      ]) {
        expect(text, `${path} names ${word}`).not.toContain(word);
      }
    }
  });

  it("wizard_offers_no_automation_in_source", () => {
    const banned = /\b(autoInstall|autoUpdate|autoApprove|schedule|recurring|unattended|policy)\b/;
    for (const { path, text } of sources.filter(({ path }) =>
      path.startsWith("features/packages/"),
    )) {
      expect(text, `${path} names an automation control`).not.toMatch(banned);
    }
  });
});
