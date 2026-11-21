import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PRESENTATION, PRESENTATION_WORDS } from "../../design/visibility";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { routeSettingsPage, settings } from "../../test/settingsFixture";
import { SettingsPage } from "./SettingsPage";
import { DisplaySection } from "./views/DisplaySection";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const answer = (profile: string, pages: Record<string, string> = {}) => ({
  version: 1,
  updated_at: null,
  presentation: { profile, pages },
});

const puts = () => calls.filter((call) => call.route === "PUT /api/v1/me/preferences");

function echo() {
  route("PUT /api/v1/me/preferences", (init) => ({
    ...answer("detailed"),
    presentation: (init.body as { presentation: object }).presentation,
  }));
}

beforeEach(() => resetApi());

function show() {
  return renderAt("/settings/display", "/settings/:section?", <DisplaySection />);
}

test("display_section_shows_the_saved_profile_and_the_table", async () => {
  route("GET /api/v1/me/preferences", () => answer("standard"));
  show();
  expect(await screen.findByLabelText("Standard")).toBeChecked();
  const table = screen.getByRole("table", { name: "What each detail level shows" });
  const p0 = within(table).getByRole("row", { name: /Safety and state/ });
  expect(within(p0).getAllByText("Shown")).toHaveLength(3);
  const p3 = within(table).getByRole("row", { name: /Metadata/ });
  expect(within(p3).getByText(PRESENTATION_WORDS[PRESENTATION.essential.P3])).toBeVisible();
});

test("choosing_a_profile_saves_it_keeping_the_page_overrides_and_says_so", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/me/preferences", () =>
    answer("detailed", { operation: "essential", future: "standard" }),
  );
  echo();
  show();
  await user.click(await screen.findByLabelText("Standard"));
  expect(await screen.findByText("Detail level set to Standard.")).toBeVisible();
  expect(puts()[0]?.init.body).toEqual({
    presentation: { profile: "standard", pages: { operation: "essential", future: "standard" } },
  });
});

test("choosing_a_page_override_sets_or_clears_that_key_only", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/me/preferences", () => answer("standard", { operation: "essential" }));
  echo();
  show();
  await user.selectOptions(await screen.findByLabelText("Operations list"), "detailed");
  await user.selectOptions(screen.getByLabelText("Operation detail"), "");
  expect(puts()[0]?.init.body).toEqual({
    presentation: {
      profile: "standard",
      pages: { operation: "essential", operations: "detailed" },
    },
  });
  expect(puts()[1]?.init.body).toEqual({
    presentation: { profile: "standard", pages: { operations: "detailed" } },
  });
});

test("reset_sends_detailed_with_no_overrides", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/me/preferences", () => answer("essential"));
  echo();
  show();
  await user.click(await screen.findByRole("button", { name: "Reset to Detailed" }));
  expect(puts()[0]?.init.body).toEqual({ presentation: { profile: "detailed", pages: {} } });
  expect(await screen.findByText("Display reset to Detailed.")).toBeVisible();
});

test("reset_is_disabled_with_a_reason_when_the_defaults_are_in_use", async () => {
  route("GET /api/v1/me/preferences", () => answer("detailed"));
  show();
  const reset = await screen.findByRole("button", { name: "Reset to Detailed" });
  expect(reset).toHaveAttribute("aria-disabled", "true");
});

test("a_failed_save_says_so_and_keeps_the_shown_value", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/me/preferences", () => answer("detailed"));
  route("PUT /api/v1/me/preferences", () => failure(400, "bad_request", "unknown profile"));
  show();
  await user.click(await screen.findByLabelText("Essential"));
  expect(await screen.findByText(/save the display setting/i)).toBeVisible();
  expect(screen.getByLabelText("Detailed")).toBeChecked();
});

test("a_failed_load_is_a_load_error_not_the_defaults", async () => {
  route("GET /api/v1/me/preferences", () => failure(500, "internal", "boom"));
  show();
  expect(await screen.findByText(/load display settings/i)).toBeVisible();
  expect(screen.queryByLabelText("Detailed")).toBeNull();
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
});

test("display_is_reachable_in_a_file_managed_hub", async () => {
  routeSettingsPage();
  route("GET /api/v1/settings", () => settings(null));
  route("GET /api/v1/me/preferences", () => answer("detailed"));
  renderAt("/settings", "/settings/:section?", <SettingsPage />);
  expect(await screen.findByRole("region", { name: "Display" })).toBeVisible();
  expect(await screen.findByLabelText("Standard")).toBeVisible();
});

test("display_is_a_section_in_the_editable_settings", async () => {
  routeSettingsPage();
  route("GET /api/v1/settings", () => settings());
  route("GET /api/v1/me/preferences", () => answer("detailed"));
  renderAt("/settings/display", "/settings/:section?", <SettingsPage />);
  const nav = await screen.findByRole("navigation", { name: "Settings sections" });
  expect(within(nav).getByRole("link", { name: /^Display/ })).toHaveAttribute(
    "aria-current",
    "page",
  );
  expect(await screen.findByLabelText("Essential")).toBeVisible();
});
