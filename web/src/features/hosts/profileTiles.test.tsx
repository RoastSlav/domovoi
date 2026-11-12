import { screen } from "@testing-library/react";
import { resetApi, route } from "../../test/apiMock";
import { T0 } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { ProfileTab } from "./ProfileTab";
import { columnsFor, readOverview } from "./profileOverview";

vi.mock("../../api/client", () => import("../../test/apiMock"));

function show() {
  renderAt("/hosts/nas", "/hosts/:name", <ProfileTab host="nas" connected />);
}

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts/{name}/profile", () => ({
    collected_at: T0,
    markdown: [
      "# Host profile: nas",
      "",
      "Agent version 0.6.0.",
      "",
      "## Summary",
      "",
      "Debian 12 on x86_64, kernel 6.8.0",
      "4 logical CPUs, 16 GiB memory",
      "no containers",
      "",
      "## Details",
      "",
      "### Overview",
      "",
      "```",
      "uptime: 41 days",
      "load: 0.08 0.11 0.09",
      "memory: 31 GiB total, 6.2 GiB used",
      "disks:",
      "  /srv 2.0T 91%",
      "  /boot 1.0G 40%",
      "```",
    ].join("\n"),
    sections: [{ title: "Overview", ok: true }],
  }));
});

test("the_overview_becomes_tiles_for_uptime_load_memory_and_each_disk_and_its_block_goes_away", async () => {
  show();
  await screen.findByText("0.6.0");
  expect(screen.getByText("41 days")).toBeVisible();
  expect(screen.getByText("0.08")).toBeVisible();
  expect(screen.getByText("6.2 GiB used")).toBeVisible();
  expect(screen.getByText("Disk /srv").closest("div")).toHaveTextContent("91% used");
  expect(screen.getByText("Disk /boot").closest("div")).toHaveTextContent("40% used");
  expect(screen.queryByRole("group", { name: "Overview" })).toBeNull();
});

test("a_disk_tile_is_toned_by_how_full_the_disk_is", () => {
  const { tiles } = readOverview("  /a 1T 84%\n  /b 1T 85%\n  /c 1T 95%");
  expect(tiles.map((tile) => tile.tone)).toEqual([undefined, "warning", "danger"]);
});

test("overview_lines_that_are_not_understood_stay_in_the_overview_card", () => {
  const { tiles, rest } = readOverview("uptime: 3 days\nfans: loud");
  expect(tiles).toHaveLength(1);
  expect(rest).toBe("fans: loud");
});

test("tile_columns_fill_even_rows_for_six_tiles_at_every_width", () => {
  expect(columnsFor(6, 6)).toBe(6);
  expect(columnsFor(6, 3)).toBe(3);
  expect(columnsFor(6, 2)).toBe(2);
  expect(columnsFor(10, 6)).toBe(5);
  expect(columnsFor(7, 6)).toBe(4);
});
