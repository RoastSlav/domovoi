import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { routeSettingsPage, settings } from "../../test/settingsFixture";
import { SettingsPage } from "./SettingsPage";
import { isSensitive, sensitiveReason } from "./form";

vi.mock("../../api/client", () => import("../../test/apiMock"));
vi.mock("../../api/health", () => ({ waitForHealthy: () => Promise.resolve(true) }));

const section = (id: string) =>
  renderAt(`/settings/${id}`, "/settings/:section?", <SettingsPage />);

beforeEach(() => {
  resetApi();
  routeSettingsPage();
  route("GET /api/v1/settings", () => settings());
});

test("retention_is_a_section_with_nine_period_fields_and_the_kept_list", async () => {
  section("retention");
  const region = await screen.findByRole("region", { name: "Retention" });
  expect(await within(region).findAllByLabelText(/\(days\)$/)).toHaveLength(9);
  expect(within(region).getByText("Operations and their summaries")).toBeVisible();
  expect(within(region).getByRole("link", { name: "Audit log" })).toHaveAttribute("href", "/audit");
  expect(within(region).getByText(/7 nightly and 3 before upgrades/)).toBeVisible();
});

test("changing_a_retention_period_asks_for_the_password_before_any_request", async () => {
  const user = userEvent.setup();
  route("PATCH /api/v1/settings", () => ({ revision: 4, applied: true }));
  section("retention");
  const field = await screen.findByLabelText("Signals (days)");
  await user.clear(field);
  await user.type(field, "14");
  await user.click(screen.getByRole("button", { name: "Save" }));
  const dialog = await screen.findByRole("dialog", { name: "Confirm with your password" });
  expect(within(dialog).getByText(/A shorter period deletes data sooner/)).toBeVisible();
  expect(calls.filter((c) => c.route === "PATCH /api/v1/settings")).toHaveLength(0);
  await user.type(within(dialog).getByLabelText("Password"), "right password!");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  await screen.findByText("Back online");
  const sent = calls.find((c) => c.route === "PATCH /api/v1/settings");
  expect(sent?.init.body).toEqual({
    base_revision: 3,
    changes: { retention: { signals_days: 14 } },
    password: "right password!",
  });
});

test("a_wrong_retention_password_keeps_the_dialog_and_says_so", async () => {
  const user = userEvent.setup();
  route("PATCH /api/v1/settings", () => failure(403, "forbidden", "wrong password"));
  section("retention");
  const field = await screen.findByLabelText("Signals (days)");
  await user.clear(field);
  await user.type(field, "14");
  await user.click(screen.getByRole("button", { name: "Save" }));
  const dialog = await screen.findByRole("dialog", { name: "Confirm with your password" });
  await user.type(within(dialog).getByLabelText("Password"), "nope");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(await within(dialog).findByText("That password is not right.")).toBeVisible();
});

test("backup_count_fields_ask_for_the_password", async () => {
  const user = userEvent.setup();
  section("backups");
  const field = await screen.findByLabelText("Pre-upgrade backups to keep");
  await user.clear(field);
  await user.type(field, "5");
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("dialog", { name: "Confirm with your password" })).toBeVisible();
});

test("a_file_managed_hub_shows_the_retention_values_read_only", async () => {
  route("GET /api/v1/settings", () => settings(null));
  section("");
  expect(await screen.findAllByText("Not applied yet")).toHaveLength(2);
  expect(screen.getByText("Applies once operations exist.")).toBeVisible();
  expect(screen.getAllByText("30 days").length).toBeGreaterThan(0);
  expect(screen.getByText("Pre-upgrade backups kept")).toBeVisible();
});

test("a_hub_that_sends_no_retention_block_shows_the_fallback", async () => {
  route("GET /api/v1/settings", () => {
    const rest: Record<string, unknown> = { ...settings(null) };
    delete rest.retention;
    return rest;
  });
  section("");
  expect(await screen.findByText("This hub did not report its retention settings.")).toBeVisible();
});

test("is_sensitive_covers_retention_and_backup_counts_and_keeps_the_old_rules", () => {
  expect(isSensitive({ retention: { signals_days: 5 } })).toBe(true);
  expect(isSensitive({ backup: { keep: 5 } })).toBe(true);
  expect(isSensitive({ backup: { keep_pre_migrate: 5 } })).toBe(true);
  expect(isSensitive({ backup: { dir: "/x" } })).toBe(false);
  expect(isSensitive({ hub: { name: "x" } })).toBe(false);
  expect(isSensitive({ web: { trusted_proxies: ["1.1.1.1"] } })).toBe(true);
  expect(sensitiveReason({ retention: { signals_days: 5 } })).toMatch(/deletes data sooner/);
  expect(
    sensitiveReason({ retention: { signals_days: 5 }, web: { trusted_proxies: ["1.1.1.1"] } }),
  ).toMatch(/who can act/);
});
