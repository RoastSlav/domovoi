import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { FirstRun } from "./FirstRun";

vi.mock("../../api/client", () => import("../../test/apiMock"));
vi.mock("../../api/health", () => ({ waitForHealthy: () => Promise.resolve(true) }));

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => []);
  route("GET /api/v1/settings", () => ({
    editable: {
      revision: 0,
      values: { hub: { name: "domovoi" } },
      fields: {},
      issues: [],
      last_apply: null,
    },
  }));
  route("PATCH /api/v1/settings", () => ({ revision: 1, applied: true }));
});

test("first_run_stepper_skips_optional_steps_and_applies_once", async () => {
  const user = userEvent.setup();
  renderAt("/welcome", "/welcome", <FirstRun />);
  expect(await screen.findByRole("heading", { name: "Hub" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Skip" })).toBeNull();
  expect(screen.getByLabelText("Public URL")).toHaveValue(window.location.origin);
  await user.clear(screen.getByLabelText("Name"));
  await user.type(screen.getByLabelText("Name"), "home");
  await user.click(screen.getByRole("button", { name: "Next" }));

  expect(screen.getByRole("heading", { name: "Discord" })).toBeVisible();
  expect(screen.getByRole("link", { name: "Discord developer portal" })).toHaveAttribute(
    "href",
    "https://discord.com/developers/applications",
  );
  expect(screen.getByLabelText("Users who may approve")).toHaveAccessibleDescription(
    /usernames, display names and tags do not work/,
  );
  await user.click(screen.getByRole("button", { name: "Set Bot token" }));
  await user.type(screen.getByLabelText("New Bot token"), "typed-then-skipped");
  await user.click(screen.getByRole("button", { name: "Skip" }));
  expect(screen.getByRole("heading", { name: "Model" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Skip" }));
  expect(screen.getByRole("heading", { name: "Digest and quiet hours" })).toBeVisible();
  await user.type(screen.getByLabelText("Digest time"), "08:30");
  expect(calls.filter((c) => c.route === "PATCH /api/v1/settings")).toHaveLength(0);
  await user.click(screen.getByRole("button", { name: "Save and apply" }));

  expect(await screen.findByRole("heading", { name: "Add your first host" })).toBeVisible();
  const saves = calls.filter((c) => c.route === "PATCH /api/v1/settings");
  expect(saves).toHaveLength(1);
  const body = saves[0]?.init.body as {
    base_revision: number;
    changes: Record<string, unknown>;
    password?: string;
  };
  expect(body.base_revision).toBe(0);
  expect(body.password).toBeUndefined();
  expect(body.changes).toEqual({
    hub: { name: "home", timezone: Intl.DateTimeFormat().resolvedOptions().timeZone },
    web: { public_url: window.location.origin },
    notify: { digest: { time: "08:30" } },
  });
  expect(JSON.stringify(body)).not.toContain("typed-then-skipped");
  await user.click(screen.getByRole("button", { name: "Finish" }));
  expect(await screen.findByTestId("location")).toHaveTextContent("/incidents");
});

test("first_run_discord_test_sends_a_scalar_channel_id_and_shows_failures", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/settings/test/discord", () =>
    failure(400, "invalid", "discord.channel_id must be a string"),
  );
  renderAt("/welcome", "/welcome", <FirstRun />);
  expect(await screen.findByRole("heading", { name: "Hub" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Next" }));

  await user.click(screen.getByRole("button", { name: "Set Bot token" }));
  await user.type(screen.getByLabelText("New Bot token"), "test-token");
  await user.type(screen.getByLabelText("Channel ID"), "123456789012345678");
  await user.type(screen.getByLabelText("Users who may approve"), "234567890123456789");
  await user.click(screen.getByRole("button", { name: "Test" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("discord.channel_id must be a string");
  const sent = calls.find((c) => c.route === "POST /api/v1/settings/test/discord");
  const body = sent?.init.body as { changes: { discord: unknown } };
  expect(body.changes.discord).toEqual({
    bot_token: { set: "test-token" },
    channel_id: "123456789012345678",
    allowed_user_ids: ["234567890123456789"],
    enabled: true,
  });
  expect(body.changes).not.toHaveProperty("triage");
});

test("first_run_test_sends_only_the_fields_of_the_step_being_tested", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/settings/test/llm", () => ({ ok: true, message: "model answered" }));
  renderAt("/welcome", "/welcome", <FirstRun />);
  expect(await screen.findByRole("heading", { name: "Hub" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Next" }));
  await user.click(screen.getByRole("button", { name: "Skip" }));
  await user.selectOptions(screen.getByLabelText("Provider"), "openai_compatible");
  await user.type(screen.getByLabelText("Base URL"), "http://localhost:11434/v1");
  await user.click(screen.getByRole("button", { name: "Test" }));

  expect(await screen.findByText(/model answered/)).toBeVisible();
  const sent = calls.find((c) => c.route === "POST /api/v1/settings/test/llm");
  const body = sent?.init.body as { changes: Record<string, unknown> };
  expect(body.changes).not.toHaveProperty("discord");
  expect(body.changes).not.toHaveProperty("hub");
});

test("first_run_secret_draft_survives_navigation_and_reports_pending", async () => {
  const user = userEvent.setup();
  renderAt("/welcome", "/welcome", <FirstRun />);
  expect(await screen.findByRole("heading", { name: "Hub" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Next" }));

  await user.click(screen.getByRole("button", { name: "Set Bot token" }));
  await user.type(screen.getByLabelText("New Bot token"), "draft-token");
  expect(screen.getByText("Will be set")).toBeVisible();
  await user.click(screen.getByRole("button", { name: /^Set$/ }));
  expect(screen.queryByLabelText("New Bot token")).toBeNull();

  await user.click(screen.getByRole("button", { name: "Next" }));
  expect(screen.getByRole("heading", { name: "Model" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Back" }));
  expect(screen.getByLabelText("New Bot token")).toHaveValue("draft-token");
  expect(screen.getByText("Will be set")).toBeVisible();
});

test("first_run_model_setup_fetches_models_and_shows_only_provider_fields", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/settings/models", () => ({ models: ["local-model"] }));
  renderAt("/welcome", "/welcome", <FirstRun />);
  expect(await screen.findByRole("heading", { name: "Hub" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Next" }));
  await user.click(screen.getByRole("button", { name: "Skip" }));

  expect(screen.queryByLabelText("Base URL")).toBeNull();
  expect(screen.queryByLabelText("CLI preset")).toBeNull();
  await user.selectOptions(screen.getByLabelText("Provider"), "openai_compatible");
  expect(screen.getByLabelText("Base URL")).toBeVisible();
  expect(screen.queryByLabelText("CLI preset")).toBeNull();
  await user.type(screen.getByLabelText("Base URL"), "http://localhost:11434/v1");
  await user.click(screen.getByRole("button", { name: "Fetch models" }));
  expect(await screen.findByText(/1 model available/)).toBeVisible();

  const sent = calls.find((c) => c.route === "POST /api/v1/settings/models");
  const body = sent?.init.body as { changes: { llm: { primary: Record<string, unknown> } } };
  expect(body.changes.llm.primary).toEqual({
    kind: "openai_compatible",
    base_url: "http://localhost:11434/v1",
  });
  await user.selectOptions(screen.getByLabelText("Provider"), "cli");
  expect(screen.getByLabelText("CLI preset")).toBeVisible();
  expect(screen.queryByRole("textbox", { name: "Model" })).toBeNull();
  expect(screen.queryByLabelText("Base URL")).toBeNull();
  expect(screen.queryByText("API key")).toBeNull();
});
