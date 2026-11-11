import { findSection } from "../settings/sections";
import { STEPS } from "./steps";

test("first_run_summary_links_point_at_real_settings_sections", () => {
  for (const step of STEPS) {
    expect(
      findSection(step.section),
      `${step.title} links to /settings/${step.section}`,
    ).toBeDefined();
  }
});
