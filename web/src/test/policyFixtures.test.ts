import { POLICY_FIXTURES } from "./policyFixtures";

const OVERVIEW = ["notes", "shadow", "shadow_enabled", "time_window_zone"];
const PREVIEW = [
  "base_number",
  "blocking",
  "locked_changes",
  "notes",
  "preview_hash",
  "rules_hash",
  "validation",
];
const DETAIL = [
  "candidate",
  "contract",
  "degraded",
  "evaluated_at",
  "execution_id",
  "gaps",
  "host_id",
  "operation_id",
  "policy_version",
  "status",
];

/** Top-level keys the generated types require, by fixture. */
const REQUIRED: Record<keyof typeof POLICY_FIXTURES, string[]> = {
  "overview-active": OVERVIEW,
  "overview-fresh": OVERVIEW,
  "overview-berlin": OVERVIEW,
  "version-3": ["acknowledgements", "rules", "summary"],
  "draft-4": ["revision", "rules", "rules_hash", "updated_at", "updated_by"],
  "preview-clean": PREVIEW,
  "preview-blocking": PREVIEW,
  "preview-invalid": PREVIEW,
  "simulate-both": ["items", "label", "notes", "summary", "truncated"],
  "shadow-page": ["items"],
  "decision-locked": DETAIL,
  "decision-exception": DETAIL,
  "decision-limits": DETAIL,
  "decision-unavailable": DETAIL,
  "decision-unevaluable": DETAIL,
  "decision-error": DETAIL,
};

describe("policy fixtures", () => {
  it("fixtures_carry_every_key_of_the_contract_examples", () => {
    for (const [name, fixture] of Object.entries(POLICY_FIXTURES)) {
      for (const key of REQUIRED[name as keyof typeof POLICY_FIXTURES]) {
        expect(fixture, `${name} lacks ${key}`).toHaveProperty(key);
      }
    }
  });
});
