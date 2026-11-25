const own = import.meta.glob("./*.{ts,tsx}", {
  query: "?raw",
  eager: true,
  import: "default",
}) as Record<string, string>;
const isTest = (path: string) => /\.test\.tsx?$/.test(path);

const everything = import.meta.glob("../../**/*.{ts,tsx}", {
  query: "?raw",
  eager: true,
  import: "default",
}) as Record<string, string>;

/** Source a person could read in the shipped bundle: no tests, fixtures, generated types or demos. */
const shipped = Object.entries(everything).filter(
  ([path]) =>
    !isTest(path) &&
    !/\/(test|dev)\//.test(path) &&
    !path.endsWith("schema.d.ts") &&
    !path.endsWith("openapi.json"),
);
const helperViews = Object.entries(own).filter(([path]) => /\/Helper|helperText/.test(path));
const featureFiles = shipped.filter(([path]) => path.includes("/features/"));

/** The four helper endpoints, as three paths (the key path takes PUT and DELETE). */
const HELPER_PATHS = [
  "/api/v1/hosts/{name}/helper",
  "/api/v1/hosts/{name}/helper/key",
  "/api/v1/hosts/{name}/helper/recheck",
];

test("helper_views_use_only_helper_endpoints", () => {
  const named = new Set<string>();
  for (const [, text] of shipped) {
    for (const match of text.matchAll(/\/api\/v1\/[^"'`\s]*helper[^"'`\s]*/g)) {
      named.add(match[0]);
    }
  }
  expect([...named].sort()).toEqual(HELPER_PATHS);
  expect(helperViews.length).toBeGreaterThan(5);
  for (const [path, text] of helperViews) {
    for (const word of ["api.GET", "api.POST", "api.PUT", "api.DELETE", "useMutation", "fetch("]) {
      expect(text.includes(word), `${path} reaches the hub itself with ${word}`).toBe(false);
    }
  }
  const writes = shipped.filter(([, text]) => /api\.(POST|PUT|DELETE)\([^)]*helper/s.test(text));
  expect(writes.map(([path]) => path.split("/").pop())).toEqual(["helper.ts"]);
  // No raw reason code or error sentence in a feature file: those come from design/.
  const tokens = [
    ...["key_not_pinned", "key_mismatch", "attestation_missing", "attestation_stale"],
    ...["signature_invalid", "report_incomplete", "generation_regressed", "path_uncovered"],
    ...["confirmation_stale", "helper_state_permissions", "state_missing", "state_corrupt"],
    ...["state_key_mismatch", "key_missing", "uninitialized", "password_required"],
    ...["wrong_password", "no_helper_key", "fingerprint_mismatch", "recheck_too_soon"],
  ];
  for (const [path, text] of featureFiles) {
    for (const token of tokens) {
      expect(text.includes(token), `${path} writes the code ${token}`).toBe(false);
    }
  }
});

test("helper_views_never_claim_verified_without_a_verified_report", () => {
  // Only `hasVerifiedReport` in helperText.ts may compare the verdict with "verified".
  for (const [path, text] of featureFiles) {
    if (path.endsWith("/helperText.ts")) {
      continue;
    }
    expect(/attestation\.state|"verified"/.test(text), `${path} reads the verdict itself`).toBe(
      false,
    );
  }
  const text = own["./helperText.ts"] ?? "";
  expect(text).toMatch(/state === "verified" && !!view\.verified && view\.verified\.fresh/);
});

test("limits_tab_has_no_write_path", () => {
  for (const [path, text] of Object.entries(own)) {
    if (!/\/Limits|limitsText/.test(path) || isTest(path)) {
      continue;
    }
    for (const word of ["useMutation", ".mutate(", "api.POST", "api.PUT", "api.DELETE"]) {
      expect(text.includes(word), `${path} contains ${word}`).toBe(false);
    }
  }
});

test("capability_center_has_no_authority_controls", () => {
  for (const [path, text] of Object.entries(own)) {
    // The helper views pin, unpin and re-check through their own hooks; `helper_views_use_only_helper_endpoints` guards them.
    if (isTest(path) || /\/Helper/.test(path)) {
      continue;
    }
    for (const word of ["api.POST", "api.PUT", "api.DELETE", ".mutate(", "useMutation"]) {
      expect(text.includes(word), `${path} contains ${word}`).toBe(false);
    }
  }
});
