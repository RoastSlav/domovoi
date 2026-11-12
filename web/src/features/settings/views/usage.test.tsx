import { render, screen } from "@testing-library/react";
import type { Settings } from "../../../api/types";
import { Usage } from "./Usage";

const settings = {
  triage: { monthly_budget_usd: 10 },
  usage: { tokens_in: 12000, tokens_out: 3400, cost_usd: 2.5 },
} as Settings;

test("the_four_usage_tiles_sit_in_one_container_so_they_wrap_as_a_set", () => {
  render(<Usage settings={settings} />);

  const tiles = ["Spent this month", "Monthly budget", "Tokens in", "Tokens out"].map((label) =>
    screen.getByText(label),
  );
  const container = tiles[0]?.closest("[class*='tiles']");
  expect(container).not.toBeNull();
  for (const tile of tiles) {
    expect(container).toContainElement(tile);
  }
  expect(container?.children).toHaveLength(4);
});
