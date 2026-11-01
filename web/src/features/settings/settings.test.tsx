import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { SettingsPage } from "./SettingsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));
vi.mock("../../api/health", () => ({ waitForHealthy: () => Promise.resolve(true) }));

function settings(editable: Record<string, unknown>) {
  return {
    hub: {
      name: "home",
      timezone: "UTC",
      bind: "127.0.0.1:7420",
      public_url: null,
      trusted_proxies: [],
    },
    llm_primary: null,
    llm_secondary: null,
    triage: { enabled: false, concurrency: 2, per_hour_runs: 6, monthly_budget_usd: null },
    discord: null,
    webhooks: [],
    heartbeat: { configured: false, url: "[unset]", interval: null, failing: false },
    usage: { tokens_in: 0, tokens_out: 0, cost_usd: 0 },
    editable: {
      revision: 3,
      values: {
        hub: { name: "home", timezone: "UTC" },
        notify: { digest: { enabled: true, time: "09:00" } },
        incidents: { confirm_runs: 2 },
        discord: { enabled: false, channel_id: "1234567890123456789", bot_token: "[set]" },
      },
      fields: { "hub.name": "file", "triage.concurrency": "env:DOMOVOI_TRIAGE__CONCURRENCY" },
      issues: [],
      last_apply: null,
      ...editable,
    },
  };
}

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/advisories", () => []);
  route("GET /api/v1/settings/schema", () => ({
    properties: { incidents: { $ref: "#/$defs/Incidents" } },
    $defs: {
      Incidents: {
        properties: {
          confirm_runs: { description: "Consecutive failing runs before an incident opens." },
        },
      },
    },
  }));
});

function section(name: string) {
  return within(screen.getByRole("region", { name }));
}

test("settings_form_shows_field_errors_from_api_paths", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/settings", () => settings({}));
  route("PATCH /api/v1/settings", () =>
    failure(400, "invalid", "the settings are not valid", undefined, [
      { path: "incidents.confirm_runs", message: "must be at least 1", severity: "error" },
      { path: "incidents", message: "a whole-section problem", severity: "error" },
    ]),
  );
  renderAt("/settings", "/settings", <SettingsPage />);
  const incidents = await screen.findByRole("region", { name: "Incidents" });
  const field = within(incidents).getByLabelText("Failing runs before an incident opens");
  expect(
    await within(incidents).findByText("Consecutive failing runs before an incident opens."),
  ).toBeVisible();
  await user.clear(field);
  await user.type(field, "0");
  await user.click(within(incidents).getByRole("button", { name: "Save and apply" }));
  const alerts = await within(incidents).findAllByRole("alert");
  expect(alerts.map((a) => a.textContent).join(" ")).toContain("must be at least 1");
  expect(field).toHaveAttribute("aria-invalid", "true");
  expect(alerts.map((a) => a.textContent).join(" ")).toContain(
    "incidents: a whole-section problem",
  );
  const sent = calls.find((c) => c.route === "PATCH /api/v1/settings");
  expect(sent?.init.body).toEqual({
    base_revision: 3,
    changes: { incidents: { confirm_runs: 0 } },
  });
});

test("locked_fields_are_read_only_with_source_label", async () => {
  route("GET /api/v1/settings", () => settings({}));
  renderAt("/settings", "/settings", <SettingsPage />);
  const hub = await screen.findByRole("region", { name: "Hub" });
  expect(within(hub).getByLabelText("Name")).toHaveAttribute("readonly");
  expect(within(hub).getByText("Set in hub.toml")).toBeVisible();
  const triage = section("Models and triage");
  expect(triage.getByLabelText("Runs at once")).toHaveAttribute("readonly");
  expect(triage.getByText("Set by DOMOVOI_TRIAGE__CONCURRENCY")).toBeVisible();
  expect(
    section("Incidents").getByLabelText("Failing runs before an incident opens"),
  ).not.toHaveAttribute("readonly");
});

test("secret_inputs_never_prefill_and_send_keep_by_default", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/settings", () => settings({}));
  route("PATCH /api/v1/settings", () => ({ revision: 4, applied: true }));
  renderAt("/settings", "/settings", <SettingsPage />);
  const discord = await screen.findByRole("region", { name: "Discord" });
  expect(within(discord).getByText("Set")).toBeVisible();
  expect(within(discord).queryByDisplayValue("[set]")).toBeNull();
  expect(within(discord).getByLabelText("Channel id")).toHaveValue("1234567890123456789");
  await user.click(within(discord).getByLabelText("Run the bot"));
  await user.click(within(discord).getByRole("button", { name: "Save and apply" }));
  expect(await within(discord).findByText("Saved and applied.")).toBeVisible();
  const sent = calls.find((c) => c.route === "PATCH /api/v1/settings");
  expect(sent?.init.body).toEqual({ base_revision: 3, changes: { discord: { enabled: true } } });

  await user.click(within(discord).getByRole("button", { name: "Replace Bot token" }));
  const input = within(discord).getByLabelText("New Bot token");
  expect(input).toHaveValue("");
  expect(input).toHaveAttribute("type", "password");
});

test("sensitive_save_prompts_for_password", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/settings", () => settings({}));
  route("PATCH /api/v1/settings", (init) => {
    const body = init.body as { password?: string };
    return body.password === "right password!"
      ? { revision: 4, applied: true }
      : failure(403, "forbidden", "wrong password");
  });
  renderAt("/settings", "/settings", <SettingsPage />);
  const discord = await screen.findByRole("region", { name: "Discord" });
  await user.click(within(discord).getByRole("button", { name: "Replace Bot token" }));
  await user.type(within(discord).getByLabelText("New Bot token"), "new-bot-token");
  await user.click(within(discord).getByRole("button", { name: "Save and apply" }));
  const dialog = await screen.findByRole("dialog", { name: "Confirm with your password" });
  expect(calls.filter((c) => c.route === "PATCH /api/v1/settings")).toHaveLength(0);
  await user.type(within(dialog).getByLabelText("Password"), "wrong");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(await within(dialog).findByRole("alert")).toHaveTextContent("That password is not right.");
  await user.clear(within(dialog).getByLabelText("Password"));
  await user.type(within(dialog).getByLabelText("Password"), "right password!");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(await within(discord).findByText("Saved and applied.")).toBeVisible();
  const last = calls.filter((c) => c.route === "PATCH /api/v1/settings").pop();
  expect(last?.init.body).toEqual({
    base_revision: 3,
    changes: { discord: { bot_token: { set: "new-bot-token" } } },
    password: "right password!",
  });
});

test("stale_revision_offers_a_reload", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/settings", () => settings({}));
  route("PATCH /api/v1/settings", () =>
    failure(409, "conflict", "settings changed elsewhere (now revision 4); reload and try again"),
  );
  renderAt("/settings", "/settings", <SettingsPage />);
  const incidents = await screen.findByRole("region", { name: "Incidents" });
  await user.type(within(incidents).getByLabelText("Reopen window"), "2h");
  await user.click(within(incidents).getByRole("button", { name: "Save and apply" }));
  expect(await within(incidents).findByText("Settings changed elsewhere, reload.")).toBeVisible();
  expect(within(incidents).getByRole("button", { name: "Reload" })).toBeVisible();
});
