const own = import.meta.glob("./*.{ts,tsx}", {
  query: "?raw",
  eager: true,
  import: "default",
}) as Record<string, string>;
const api = import.meta.glob("../../api/**/*.{ts,tsx}", {
  query: "?raw",
  eager: true,
  import: "default",
}) as Record<string, string>;

const isTest = (path: string) => /\.test\.tsx?$/.test(path);

test("capability_center_has_no_helper_data_path", () => {
  const words = [
    "/api/v1/helper",
    "helper/enroll",
    "attest",
    "safety_attestation",
    "helper_status",
    "enroll_helper",
  ];
  const files = [
    ...Object.entries(api).filter(([path]) => !path.endsWith("schema.d.ts") && !isTest(path)),
    ...Object.entries(own).filter(([path]) => /\/Helper/.test(path) && !isTest(path)),
  ];
  expect(files.length).toBeGreaterThan(5);
  for (const [path, text] of files) {
    for (const word of words) {
      expect(
        text.includes(word),
        `${path} mentions ${word}: when the hub gains helper endpoints, replace the placeholder and update this test`,
      ).toBe(false);
    }
  }
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
    if (isTest(path)) {
      continue;
    }
    for (const word of ["api.POST", "api.PUT", "api.DELETE", ".mutate(", "useMutation"]) {
      expect(text.includes(word), `${path} contains ${word}`).toBe(false);
    }
  }
});
