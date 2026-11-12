import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../test/apiMock";
import { incident, nas } from "../test/fixtures";
import { renderAt } from "../test/render";
import { Layout } from "./Layout";

vi.mock("../api/client", () => import("../test/apiMock"));
vi.mock("../api/events", async (original) => ({
  ...(await original<typeof import("../api/events")>()),
  useLiveEvents: () => ({ status: "live", paused: false }),
}));

const QUIET = {
  incidents_action_needed: 0,
  incidents_open: 0,
  approvals_waiting: 0,
  hosts_offline: 0,
  hosts_total: 1,
};

const listed = (route: string) => calls.filter((call) => call.route === route);

beforeEach(() => {
  resetApi();
  route("GET /api/v1/session", () => ({ user: { username: "admin" }, csrf_token: "t" }));
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/overview", () => QUIET);
  route("GET /api/v1/system/update", () => ({ current: "0.1.0", latest: null, newer: false }));
  route("GET /api/v1/incidents", () => ({
    items: [
      incident({ id: "inc-info", title: "Backup ran late", severity: "info" }),
      incident({ id: "inc-crit", title: "Disk /srv is 93% full", severity: "critical" }),
    ],
    next_cursor: null,
  }));
});

async function openPalette() {
  const user = userEvent.setup();
  renderAt("/hosts", "/*", <Layout />);
  await user.keyboard("{Control>}k{/Control}");
  return { user, search: await screen.findByRole("combobox", { name: "Search" }) };
}

test("typing_a_settings_field_label_finds_the_section_that_holds_it", async () => {
  const { user, search } = await openPalette();
  await user.type(search, "quiet hours");

  expect(await screen.findByRole("option", { name: /Notifications settings/ })).toBeVisible();
});

test("choosing_a_settings_result_opens_that_section", async () => {
  const { user, search } = await openPalette();
  await user.type(search, "discord");
  await user.click(await screen.findByRole("option", { name: /Discord settings/ }));

  expect(screen.queryByRole("combobox", { name: "Search" })).toBeNull();
  expect(screen.getByTestId("location")).toHaveTextContent("/settings/discord");
});

test("an_open_incident_appears_in_the_palette_and_opens_its_page", async () => {
  const { user, search } = await openPalette();
  await user.type(search, "93%");
  const option = await screen.findByRole("option", { name: /Disk \/srv is 93% full/ });
  expect(option).toHaveTextContent("Critical");
  await user.click(option);

  expect(screen.queryByRole("combobox", { name: "Search" })).toBeNull();
  expect(screen.getByTestId("location")).toHaveTextContent("/incidents/inc-crit");
});

test("incidents_are_not_fetched_until_the_palette_opens", async () => {
  renderAt("/hosts", "/*", <Layout />);
  expect(await screen.findByRole("combobox", { name: "Host" })).toBeVisible();
  expect(listed("GET /api/v1/incidents")).toHaveLength(0);
});
