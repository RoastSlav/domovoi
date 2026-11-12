import { render, screen } from "@testing-library/react";
import type { HostCheck } from "../../api/types";
import { CheckTable } from "./CheckTable";
import { thresholdWords } from "./thresholdText";

const check = (key: string, thresholds: Record<string, unknown>): HostCheck => ({
  key,
  reported: {
    enabled: true,
    interval_seconds: 60,
    thresholds: thresholds as Record<string, never>,
  },
  overrides: null,
});

test("thresholds_read_as_key_value_words_joined_with_dots_and_lists_with_commas", () => {
  expect(thresholdWords({ warning: 85, critical: 95 })).toBe("warning 85 · critical 95");
  expect(thresholdWords({ warn_percent: 80 })).toBe("warn_percent 80");
  expect(thresholdWords({ attributes: ["Reallocated_Sector_Ct", "Current_Pending_Sector"] })).toBe(
    "attributes Reallocated_Sector_Ct, Current_Pending_Sector",
  );
});

test("the_checks_table_never_shows_json_in_the_thresholds_column", () => {
  render(
    <CheckTable
      connected
      checks={[
        check("smart.health", { attributes: ["Reallocated_Sector_Ct", "Current_Pending_Sector"] }),
        check("disk.usage", { warning: 85, critical: 95 }),
      ]}
      drafts={{}}
      failing={new Set()}
      onEdit={() => undefined}
    />,
  );
  expect(screen.getByText("warning 85 · critical 95")).toBeVisible();
  expect(
    screen.getByText("attributes Reallocated_Sector_Ct, Current_Pending_Sector"),
  ).toBeVisible();
  expect(screen.getByRole("table", { name: "Checks" }).textContent).not.toMatch(/[[\]{}"]/);
});
