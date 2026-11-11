import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { routeSettingsPage, settings } from "../../test/settingsFixture";
import { SettingsPage } from "./SettingsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));
vi.mock("../../api/health", () => ({ waitForHealthy: () => Promise.resolve(true) }));

function renderSection(id: string) {
  return renderAt(`/settings/${id}`, "/settings/:section?", <SettingsPage />);
}

beforeEach(() => {
  resetApi();
  routeSettingsPage();
  route("GET /api/v1/settings", () => settings());
});

test("secret_inputs_never_prefill_and_send_keep_by_default", async () => {
  const user = userEvent.setup();
  route("PATCH /api/v1/settings", () => ({ revision: 4, applied: true }));
  renderSection("discord");
  const discord = await screen.findByRole("region", { name: "Discord" });
  expect(within(discord).getByText("Set")).toBeVisible();
  expect(within(discord).queryByDisplayValue("[set]")).toBeNull();
  expect(within(discord).getByLabelText("Channel ID")).toHaveValue("1234567890123456789");

  await user.click(within(discord).getByRole("switch", { name: /Run the bot/ }));
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByText("Back online")).toBeVisible();
  const sent = calls.find((c) => c.route === "PATCH /api/v1/settings");
  expect(sent?.init.body).toEqual({ base_revision: 3, changes: { discord: { enabled: true } } });
});

test("replacing_a_secret_reveals_an_empty_password_input", async () => {
  const user = userEvent.setup();
  renderSection("discord");
  const discord = await screen.findByRole("region", { name: "Discord" });
  await user.click(within(discord).getByRole("button", { name: "Replace Bot token" }));
  const input = within(discord).getByLabelText("Bot token");
  expect(input).toHaveValue("");
  expect(input).toHaveAttribute("type", "password");
});

test("discord_setup_explains_permissions_and_numeric_user_ids", async () => {
  renderSection("discord");
  const discord = await screen.findByRole("region", { name: "Discord" });
  expect(within(discord).getByRole("link", { name: "Discord developer portal" })).toHaveAttribute(
    "href",
    "https://discord.com/developers/applications",
  );
  expect(within(discord).getByRole("link", { name: "permissions documentation" })).toHaveAttribute(
    "href",
    "https://discord.com/developers/docs/topics/permissions",
  );
  expect(
    within(discord).getByText(/Create Public Threads and Send Messages in Threads/),
  ).toBeVisible();
  expect(within(discord).getByText(/Message Content Intent/)).toBeVisible();
  expect(within(discord).getByLabelText("Users who may approve")).toHaveAccessibleDescription(
    /Numeric Discord user IDs.*usernames, display names and tags do not work/,
  );
});

test("a_field_shows_the_hint_from_the_schema", async () => {
  renderSection("incidents");
  const field = await screen.findByLabelText("Failing runs before an incident opens");
  await waitFor(() =>
    expect(field).toHaveAccessibleDescription("Consecutive failing runs before an incident opens."),
  );
});

test("model_provider_fields_are_mutually_exclusive", async () => {
  const user = userEvent.setup();
  renderSection("models");
  const models = await screen.findByRole("region", { name: "Models and triage" });
  await user.selectOptions(within(models).getByLabelText("Provider"), "openai_compatible");
  expect(within(models).getByLabelText("Base URL")).toBeVisible();
  expect(within(models).getByLabelText("Model")).toBeVisible();
  expect(within(models).queryByLabelText("CLI preset")).toBeNull();
  await user.selectOptions(within(models).getByLabelText("Provider"), "cli");
  expect(within(models).getByLabelText("CLI preset")).toBeVisible();
  expect(within(models).queryByLabelText("Base URL")).toBeNull();
  expect(within(models).queryByLabelText("Model")).toBeNull();
});

test("an_input_that_cannot_be_saved_says_what_is_expected_and_blocks_the_save", async () => {
  const user = userEvent.setup();
  renderSection("incidents");
  const field = await screen.findByLabelText("Failing runs before an incident opens");
  await user.clear(field);
  await user.type(field, "many");
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findAllByText(/Enter a number\./)).not.toHaveLength(0);
  expect(field).toHaveAttribute("aria-invalid", "true");
  expect(calls.filter((c) => c.route === "PATCH /api/v1/settings")).toHaveLength(0);
});

test("sensitive_save_prompts_for_password", async () => {
  const user = userEvent.setup();
  route("PATCH /api/v1/settings", (init) => {
    const body = init.body as { password?: string };
    return body.password === "right password!"
      ? { revision: 4, applied: true }
      : failure(403, "forbidden", "wrong password");
  });
  renderSection("discord");
  const discord = await screen.findByRole("region", { name: "Discord" });
  await user.click(within(discord).getByRole("button", { name: "Replace Bot token" }));
  await user.type(within(discord).getByLabelText("Bot token"), "new-bot-token");
  await user.click(screen.getByRole("button", { name: "Save" }));
  const dialog = await screen.findByRole("dialog", { name: "Confirm with your password" });
  expect(calls.filter((c) => c.route === "PATCH /api/v1/settings")).toHaveLength(0);
  await user.type(within(dialog).getByLabelText("Password"), "wrong");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(await within(dialog).findByRole("alert")).toHaveTextContent("That password is not right.");
  await user.clear(within(dialog).getByLabelText("Password"));
  await user.type(within(dialog).getByLabelText("Password"), "right password!");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(await screen.findByText("Back online")).toBeVisible();
  const last = calls.filter((c) => c.route === "PATCH /api/v1/settings").pop();
  expect(last?.init.body).toEqual({
    base_revision: 3,
    changes: { discord: { bot_token: { set: "new-bot-token" } } },
    password: "right password!",
  });
});

test("fallback_model_section_tests_the_secondary_slot", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/settings/test/llm", () => ({
    ok: false,
    message: "no fallback model is set up",
    latency_ms: 1,
  }));
  renderSection("fallback-model");
  const fallback = await screen.findByRole("region", { name: "Fallback model" });
  expect(within(fallback).getByText(/^Used when the main model fails/)).toBeVisible();
  await user.click(within(fallback).getByRole("button", { name: "Test the fallback model" }));
  const sent = calls.find((c) => c.route === "POST /api/v1/settings/test/llm");
  expect(sent?.init.body).toEqual({ changes: {}, slot: "secondary" });
  const failed = await within(fallback).findByText("The test failed");
  expect(failed).toBeVisible();
  expect(within(fallback).getByText("no fallback model is set up")).toBeVisible();
  expect(
    within(fallback).getByText(/Check the provider, the model name and the key/),
  ).toBeVisible();
});

test("a_working_connection_test_says_what_it_checked", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/settings/test/discord", () => ({
    ok: true,
    message: "posted",
    latency_ms: 42,
  }));
  renderSection("discord");
  await user.click(await screen.findByRole("button", { name: "Send a test message" }));
  expect(await screen.findByText("Discord took a test message")).toBeVisible();
  expect(screen.getByText("posted (42 ms)")).toBeVisible();
});

test("adding_a_webhook_gives_it_fields_and_counts_nothing_until_one_is_set", async () => {
  const user = userEvent.setup();
  renderSection("webhooks");
  await user.type(await screen.findByLabelText("New webhook name"), "alerts");
  await user.click(screen.getByRole("button", { name: "Add webhook" }));
  expect(await screen.findByLabelText("alerts: kind")).toBeVisible();
  expect(screen.getByText("No unsaved changes")).toBeVisible();
});

test("the_about_section_shows_the_hub_version_and_a_summary_of_agent_versions", async () => {
  renderSection("about");
  const about = await screen.findByRole("region", { name: "About" });
  expect(await within(about).findByText("0.1.0")).toBeVisible();
  expect(await within(about).findByText("2 agents connected, 1 older than the hub.")).toBeVisible();
  expect(within(about).getByText("Update available")).toBeVisible();
});
