import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas, T0 } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { DigestPage } from "../digest/DigestPage";
import { SettingsPage } from "./SettingsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

/** A value that must never be shown, placed only in fields the API marks secret. */
const LEAKED = "sk-live-0123456789abcdef-never-show";

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
});

function settings() {
  return {
    hub: {
      name: "home",
      timezone: "Europe/Sofia",
      bind: "127.0.0.1:7420",
      public_url: "https://domovoi.example.net",
      trusted_proxies: [],
    },
    llm_primary: { kind: "anthropic", model: "claude", base_url: null, api_key: "[set]" },
    llm_secondary: { kind: "openai_compatible", model: "local", base_url: null, api_key: LEAKED },
    triage: { enabled: true, concurrency: 2, per_hour_runs: 20, monthly_budget_usd: 10 },
    discord: {
      enabled: true,
      channel_id: "123",
      allowed_user_ids: ["42"],
      bot_token: LEAKED,
      webhook_url: "[unset]",
    },
    webhooks: [
      { name: "alerts", kind: "alertmanager", auth: "bearer", secret: LEAKED, default_host: null },
    ],
    heartbeat: { configured: true, url: "[set]", interval: "5m", failing: true },
    usage: { tokens_in: 12000, tokens_out: 3400, cost_usd: 2.5 },
  };
}

test("settings_never_render_secret_values", async () => {
  route("GET /api/v1/settings", settings);
  route("GET /api/v1/advisories", () => [
    {
      id: "a1",
      kind: "stale_image",
      host_id: nas.id,
      title: "plex image is 90 days old",
      detail: "Pull a newer image.",
      created_at: T0,
    },
  ]);
  const { container } = renderAt("/settings", "/settings", <SettingsPage />);

  expect(await screen.findByRole("heading", { name: "Settings" })).toBeVisible();
  expect(container.textContent).not.toContain(LEAKED);
  expect(container.textContent).not.toContain("sk-live");

  const primary = screen.getByRole("region", { name: "Primary model" });
  expect(within(primary).getByText("set")).toBeVisible();
  const fallback = screen.getByRole("region", { name: "Fallback model" });
  expect(within(fallback).getByText("hidden"), "an unexpected value is withheld").toBeVisible();
  const discord = screen.getByRole("region", { name: "Discord" });
  expect(within(discord).getByText("not set")).toBeVisible();

  const triage = screen.getByRole("region", { name: "Triage" });
  expect(within(triage).getByText(/\$2\.50 of \$10\.00 this month/)).toBeVisible();
  expect(within(triage).getByRole("meter", { name: "Budget used this month" })).toHaveAttribute(
    "value",
    "2.5",
  );
  expect(screen.getByText("Failing")).toBeVisible();
  const advisories = screen.getByRole("region", { name: "Advisories" });
  expect(await within(advisories).findByText("plex image is 90 days old")).toBeVisible();
  expect(within(advisories).getByText(/nas/)).toBeVisible();
});

test("digest_preview_and_send_now", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/digest/preview", () => ({
    date: "2026-11-01",
    markdown: "## nas\n\n- **2 incidents** resolved",
  }));
  route("POST /api/v1/digest/send", () => failure(503, "unavailable", "no notifier"));
  renderAt("/digest", "/digest", <DigestPage />);

  const preview = await screen.findByRole("article", { name: "Digest for 2026-11-01" });
  expect(within(preview).getByRole("heading", { name: "nas" })).toBeVisible();
  expect(within(preview).getByText("2 incidents").tagName).toBe("STRONG");

  await user.click(screen.getByRole("button", { name: "Send now" }));
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Send" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("No chat is configured to send to.");

  route("POST /api/v1/digest/send", () => undefined);
  await user.click(screen.getByRole("button", { name: "Send now" }));
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Sent.")).toBeVisible();
  expect(calls.filter((c) => c.route === "POST /api/v1/digest/send")).toHaveLength(2);
});
