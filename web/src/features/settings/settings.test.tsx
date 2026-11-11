import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { routeSettingsPage, settings } from "../../test/settingsFixture";
import { SettingsPage } from "./SettingsPage";

const health = vi.hoisted(() => ({ wait: () => Promise.resolve(true) }));

vi.mock("../../api/client", () => import("../../test/apiMock"));
vi.mock("../../api/health", () => ({ waitForHealthy: () => health.wait() }));

const PATH = "/settings/:section?";

function renderSettings(url = "/settings") {
  return renderAt(url, PATH, <SettingsPage />);
}

const nav = () => screen.getByRole("navigation", { name: "Settings sections" });
const link = (name: string) => within(nav()).getByRole("link", { name: new RegExp(`^${name}`) });

beforeEach(() => {
  resetApi();
  health.wait = () => Promise.resolve(true);
  routeSettingsPage();
  route("GET /api/v1/settings", () => settings());
});

test("the_section_follows_the_url", async () => {
  const user = userEvent.setup();
  renderSettings("/settings/discord");

  expect(await screen.findByRole("region", { name: "Discord" })).toBeVisible();
  expect(link("Discord")).toHaveAttribute("aria-current", "page");
  expect(screen.queryByRole("region", { name: "Incidents" })).toBeNull();

  await user.click(link("Incidents"));
  expect(await screen.findByRole("region", { name: "Incidents" })).toBeVisible();
  expect(screen.getByTestId("location")).toHaveTextContent("/settings/incidents");
  expect(link("Incidents")).toHaveAttribute("aria-current", "page");
});

test("the_first_section_shows_at_the_bare_settings_address", async () => {
  renderSettings("/settings");
  expect(await screen.findByRole("region", { name: "Hub" })).toBeVisible();
  expect(link("Hub")).toHaveAttribute("aria-current", "page");
});

test("an_unknown_section_says_so_and_links_back", async () => {
  renderSettings("/settings/nowhere");
  expect(await screen.findByText("No such section")).toBeVisible();
  expect(screen.getByRole("link", { name: "Back to settings" })).toHaveAttribute(
    "href",
    "/settings",
  );
});

test("an_unsaved_change_puts_the_unsaved_dot_on_its_section", async () => {
  const user = userEvent.setup();
  renderSettings("/settings/incidents");
  const field = await screen.findByLabelText("Reopen window");
  expect(link("Incidents")).not.toHaveAccessibleName(/unsaved changes/);

  await user.type(field, "2h");
  expect(link("Incidents")).toHaveAccessibleName(/unsaved changes/);
  expect(link("Hub")).not.toHaveAccessibleName(/unsaved changes/);
  expect(screen.getByText("1 unsaved change")).toBeVisible();

  // Edits survive a trip to another section.
  await user.click(link("Hub"));
  await user.click(link("Incidents"));
  expect(await screen.findByLabelText("Reopen window")).toHaveValue("2h");
  expect(link("Incidents")).toHaveAccessibleName(/unsaved changes/);
});

test("a_locked_field_is_read_only_and_states_its_reason", async () => {
  renderSettings("/settings/hub");
  const name = await screen.findByLabelText("Name");
  expect(name).toHaveAttribute("readonly");
  expect(name).toHaveAccessibleDescription(/Set in hub\.toml; edit the file to change it\./);

  await userEvent.setup().click(link("Models and triage"));
  const runs = await screen.findByLabelText("Runs at once");
  expect(runs).toHaveAttribute("readonly");
  expect(runs).toHaveAccessibleDescription(
    /Set by the DOMOVOI_TRIAGE__CONCURRENCY environment variable/,
  );
});

test("every_field_says_where_its_value_comes_from", async () => {
  renderSettings("/settings/hub");
  const hub = await screen.findByRole("region", { name: "Hub" });
  expect(within(hub).getByText("from hub.toml")).toBeVisible();
  expect(within(hub).getAllByText("default").length).toBeGreaterThan(0);
});

test("a_hub_issue_focuses_the_field_from_the_summary_switching_section_if_needed", async () => {
  const user = userEvent.setup();
  route("PATCH /api/v1/settings", () =>
    failure(400, "invalid", "the settings are not valid", undefined, [
      { path: "incidents.confirm_runs", message: "must be at least 1", severity: "error" },
      {
        path: "discord.channel_id",
        message: "is not a channel the bot can see",
        severity: "error",
      },
    ]),
  );
  renderSettings("/settings/incidents");
  const field = await screen.findByLabelText("Failing runs before an incident opens");
  await user.clear(field);
  await user.type(field, "0");
  await user.click(screen.getByRole("button", { name: "Save" }));

  const summary = await screen.findByText("2 problems to fix");
  expect(summary).toBeVisible();
  expect(field).toHaveAttribute("aria-invalid", "true");
  expect(link("Incidents")).toHaveAccessibleName(/has problems/);
  expect(link("Discord")).toHaveAccessibleName(/has problems/);

  await user.click(screen.getByRole("link", { name: "is not a channel the bot can see" }));
  const channel = await screen.findByLabelText("Channel ID");
  expect(screen.getByTestId("location")).toHaveTextContent("/settings/discord");
  expect(channel).toHaveFocus();
  expect(channel).toHaveAttribute("aria-invalid", "true");

  const sent = calls.find((c) => c.route === "PATCH /api/v1/settings");
  expect(sent?.init.body).toEqual({
    base_revision: 3,
    changes: { incidents: { confirm_runs: 0 } },
  });
});

test("an_issue_for_a_whole_section_lands_on_its_first_field", async () => {
  const user = userEvent.setup();
  route("PATCH /api/v1/settings", () =>
    failure(400, "invalid", "invalid", undefined, [
      { path: "incidents", message: "a whole-section problem", severity: "error" },
      { path: "somewhere.else", message: "no field here", severity: "error" },
    ]),
  );
  renderSettings("/settings/incidents");
  await user.type(await screen.findByLabelText("Reopen window"), "2h");
  await user.click(screen.getByRole("button", { name: "Save" }));

  expect(await screen.findByText("somewhere.else: no field here")).toBeVisible();
  const field = screen.getByLabelText("Failing runs before an incident opens");
  expect(field).toHaveAccessibleDescription(/a whole-section problem/);
});

test("the_restart_flow_shows_progress_then_back_online", async () => {
  const user = userEvent.setup();
  let release: (healthy: boolean) => void = () => undefined;
  health.wait = () => new Promise<boolean>((resolve) => (release = resolve));
  route("PATCH /api/v1/settings", () => ({ revision: 4, applied: true }));
  renderSettings("/settings/incidents");
  await user.type(await screen.findByLabelText("Reopen window"), "2h");
  await user.click(screen.getByRole("button", { name: "Save" }));

  expect(await screen.findByText("Restarting the hub…")).toBeVisible();
  expect(screen.queryByText("Back online")).toBeNull();
  const before = calls.filter((c) => c.route === "GET /api/v1/session").length;

  release(true);
  expect(await screen.findByText("Back online")).toBeVisible();
  expect(calls.filter((c) => c.route === "GET /api/v1/session").length).toBeGreaterThan(before);
  expect(screen.queryByText("Restarting the hub…")).toBeNull();
});

test("a_hub_that_does_not_answer_after_a_restart_says_so_in_words", async () => {
  const user = userEvent.setup();
  health.wait = () => Promise.resolve(false);
  route("PATCH /api/v1/settings", () => ({ revision: 4, applied: true }));
  renderSettings("/settings/incidents");
  await user.type(await screen.findByLabelText("Reopen window"), "2h");
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByText("The hub has not come back yet")).toBeVisible();
});

test("a_save_the_hub_cannot_restart_for_asks_the_person_to_restart_it", async () => {
  const user = userEvent.setup();
  route("PATCH /api/v1/settings", () => ({ revision: 4, applied: false }));
  renderSettings("/settings/incidents");
  await user.type(await screen.findByLabelText("Reopen window"), "2h");
  await user.type(screen.getByLabelText("Grouping window"), "5m");
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByText("Restart the hub to apply 2 changes")).toBeVisible();
});

test("discard_asks_first_when_more_than_three_changes_are_unsaved", async () => {
  const user = userEvent.setup();
  renderSettings("/settings/incidents");
  await user.type(await screen.findByLabelText("Reopen window"), "2h");
  await user.click(screen.getByRole("button", { name: "Discard" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByLabelText("Reopen window")).toHaveValue("");

  await user.type(screen.getByLabelText("Reopen window"), "2h");
  await user.type(screen.getByLabelText("Grouping window"), "5m");
  await user.type(screen.getByLabelText("Resolve when quiet for"), "3d");
  await user.type(screen.getByLabelText("Failing runs before an incident opens"), "0");
  await user.click(screen.getByRole("button", { name: "Discard" }));
  const dialog = await screen.findByRole("dialog", { name: "Discard 4 unsaved changes?" });
  await user.click(within(dialog).getByRole("button", { name: "Discard changes" }));
  expect(screen.getByLabelText("Reopen window")).toHaveValue("");
  expect(screen.getByText("No unsaved changes")).toBeVisible();
});

test("stale_revision_offers_a_reload", async () => {
  const user = userEvent.setup();
  route("PATCH /api/v1/settings", () =>
    failure(409, "conflict", "settings changed elsewhere (now revision 4); reload and try again"),
  );
  renderSettings("/settings/incidents");
  await user.type(await screen.findByLabelText("Reopen window"), "2h");
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByText("Settings changed elsewhere")).toBeVisible();
  expect(screen.getByRole("button", { name: "Reload" })).toBeVisible();
});

test("a_settings_load_failure_offers_retry", async () => {
  route("GET /api/v1/settings", () => failure(503, "unavailable", "the store is busy"));
  renderSettings();
  expect(await screen.findByText("Could not load settings")).toBeVisible();
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
});

test("the_page_intro_explains_that_file_values_are_locked", async () => {
  renderSettings();
  await screen.findByRole("region", { name: "Hub" });
  expect(
    screen.getByText(/Fields set in hub\.toml or the environment are read-only here\./),
  ).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about settings (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/install/config-as-code.html");
});

test("read_only_settings_point_at_the_file_to_change", async () => {
  route("GET /api/v1/settings", () => settings(null));
  renderSettings();
  expect(await screen.findByText("Settings are managed by file")).toBeVisible();
  expect(
    screen.getByText("Read-only. Change settings in hub.toml and restart the hub."),
  ).toBeVisible();
  expect(screen.queryByRole("navigation", { name: "Settings sections" })).toBeNull();
});
