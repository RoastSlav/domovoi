import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { HostCheck } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { incident } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { ChecksTab } from "./ChecksTab";
import { draftOf, effectiveOf, parseDraft, problemsFor } from "./checkDraft";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const PUT = "PUT /api/v1/hosts/{name}/checks";

/** The checks a connected agent reports: one saved override, one failing, one off. */
function serve(connected = true) {
  route("GET /api/v1/hosts/{name}/checks", () => ({
    connected,
    checks: [
      {
        key: "disk.usage",
        reported: connected
          ? { enabled: true, interval_seconds: 60, thresholds: { warn_percent: 80 } }
          : null,
        overrides: { enabled: null, interval_seconds: 300, thresholds: {} },
      },
      {
        key: "memory.available",
        reported: { enabled: true, interval_seconds: 30, thresholds: {} },
        overrides: null,
      },
      {
        key: "smart.health",
        reported: { enabled: true, interval_seconds: 3600, thresholds: {} },
        overrides: { enabled: false, interval_seconds: null, thresholds: {} },
      },
    ],
  }));
  route("GET /api/v1/incidents", () => ({
    items: [incident({ check: "memory.available" })],
    next_cursor: null,
  }));
  route("GET /api/v1/hosts/{name}/docker", () => ({
    connected,
    reported: false,
    engine: "",
    endpoint: "",
    available: false,
    locked: false,
    enabled: false,
    choice: null,
    problem: null,
  }));
  route(PUT, () => ({ delivered: connected }));
}

function show() {
  renderAt("/hosts/nas", "/hosts/:name", <ChecksTab host="nas" />);
}

/** The table row of check `key`. */
async function row(key: string): Promise<HTMLElement> {
  const table = await screen.findByRole("table", { name: "Checks" });
  return within(table).getByText(key).closest("tr") as HTMLElement;
}

async function edit(user: ReturnType<typeof userEvent.setup>, key: string) {
  await user.click(await screen.findByRole("button", { name: `Edit ${key}` }));
}

beforeEach(resetApi);

test("each_check_shows_its_state_values_and_where_they_come_from", async () => {
  serve();
  show();
  const disk = await row("disk.usage");
  expect(within(disk).getByText("OK")).toBeVisible();
  expect(within(disk).getByText("300 s")).toBeVisible();
  expect(within(disk).getByText("warn_percent 80")).toBeVisible();
  expect(within(disk).getByText("Hub override")).toBeVisible();

  const memory = await row("memory.available");
  expect(within(memory).getByText("Failing")).toBeVisible();
  expect(within(memory).getByText("Agent default")).toBeVisible();

  const smart = await row("smart.health");
  expect(within(smart).getByText("Disabled")).toBeVisible();
});

test("a_check_override_saves_and_shows_a_toast", async () => {
  serve();
  const user = userEvent.setup();
  show();
  await edit(user, "disk.usage");
  const dialog = screen.getByRole("dialog", { name: "Edit disk.usage" });
  await user.selectOptions(within(dialog).getByLabelText("Runs"), "off");
  await user.clear(within(dialog).getByLabelText("Interval in seconds"));
  await user.type(within(dialog).getByLabelText("Interval in seconds"), "120");
  await user.click(within(dialog).getByLabelText("Thresholds"));
  await user.paste('{"warn_percent": 70}');
  await user.click(within(dialog).getByRole("button", { name: "Apply" }));

  expect(await screen.findByText("1 unsaved change")).toBeVisible();
  expect(within(await row("disk.usage")).getByText("Unsaved")).toBeVisible();
  expect(calls.filter((c) => c.route === PUT)).toHaveLength(0);

  await user.click(screen.getByRole("button", { name: "Save checks" }));
  expect(await screen.findByText("Checks saved and sent to the agent")).toBeVisible();
  expect(calls.find((c) => c.route === PUT)?.init.params?.path).toEqual({ name: "nas" });
  expect(calls.find((c) => c.route === PUT)?.init.body).toEqual({
    overrides: {
      "disk.usage": { enabled: false, interval_seconds: 120, thresholds: { warn_percent: 70 } },
      "smart.health": { enabled: false, interval_seconds: null, thresholds: {} },
    },
  });
  // The floating bar leaves once everything is saved.
  await waitFor(() => expect(screen.queryByText(/unsaved change/)).toBeNull());
});

test("saving_while_the_agent_is_offline_says_it_applies_when_it_reconnects", async () => {
  serve(false);
  const user = userEvent.setup();
  show();
  expect(await screen.findByText("The agent is offline")).toBeVisible();
  await edit(user, "memory.available");
  await user.selectOptions(screen.getByLabelText("Runs"), "off");
  await user.click(screen.getByRole("button", { name: "Apply" }));
  await user.click(screen.getByRole("button", { name: "Save checks" }));
  expect(
    await screen.findByText("The agent is offline. They apply when it reconnects."),
  ).toBeVisible();
});

test("a_hub_issue_marks_the_field_it_is_about", async () => {
  serve();
  route(PUT, () =>
    failure(400, "invalid", "invalid checks", undefined, [
      { path: "disk.usage.interval_seconds", message: "must be at least 5", severity: "error" },
    ]),
  );
  const user = userEvent.setup();
  show();
  await edit(user, "memory.available");
  await user.selectOptions(screen.getByLabelText("Runs"), "off");
  await user.click(screen.getByRole("button", { name: "Apply" }));
  await user.click(screen.getByRole("button", { name: "Save checks" }));

  const dialog = await screen.findByRole("dialog", { name: "Edit disk.usage" });
  const interval = within(dialog).getByLabelText("Interval in seconds");
  expect(interval).toBeInvalid();
  expect(within(dialog).getByRole("alert")).toHaveTextContent("must be at least 5");
  expect(within(dialog).getByLabelText("Thresholds")).toBeValid();
});

test("thresholds_that_are_not_json_mark_the_field_and_stage_nothing", async () => {
  serve();
  const user = userEvent.setup();
  show();
  await edit(user, "memory.available");
  await user.click(screen.getByLabelText("Thresholds"));
  await user.paste("{nope");
  await user.click(screen.getByRole("button", { name: "Apply" }));

  expect(screen.getByLabelText("Thresholds")).toBeInvalid();
  expect(screen.getByRole("alert")).toHaveTextContent(/not valid JSON/);
  expect(screen.getByRole("dialog")).toBeVisible();
  expect(screen.queryByText(/unsaved change/)).toBeNull();
});

test("discarding_drops_the_unsaved_changes", async () => {
  serve();
  const user = userEvent.setup();
  show();
  await edit(user, "memory.available");
  await user.selectOptions(screen.getByLabelText("Runs"), "off");
  await user.click(screen.getByRole("button", { name: "Apply" }));
  expect(await screen.findByText("1 unsaved change")).toBeVisible();

  await user.click(screen.getByRole("button", { name: "Discard" }));
  expect(screen.queryByText(/unsaved change/)).toBeNull();
  expect(within(await row("memory.available")).queryByText("Unsaved")).toBeNull();
});

test("checks_that_fail_to_load_show_retry_and_not_an_empty_table", async () => {
  serve();
  route("GET /api/v1/hosts/{name}/checks", () => failure(500, "internal", "boom"));
  show();
  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.queryByText("No checks yet")).toBeNull();
});

test("a_host_with_no_checks_says_why_and_what_to_look_at", async () => {
  serve();
  route("GET /api/v1/hosts/{name}/checks", () => ({ connected: false, checks: [] }));
  show();
  expect(await screen.findByText("No checks yet")).toBeVisible();
  expect(screen.getByText(/The agent is offline, so its checks are not known yet/)).toBeVisible();
});

test("a_draft_reads_back_as_the_override_it_describes", () => {
  const check = {
    key: "disk.usage",
    reported: { enabled: true, interval_seconds: 60, thresholds: { warn_percent: 80 } },
    overrides: { enabled: false, interval_seconds: 5, thresholds: { warn_percent: 70 } },
  } as unknown as HostCheck;
  const draft = draftOf(check);
  expect(draft).toEqual({ runs: "off", interval: "5", thresholds: '{"warn_percent":70}' });
  expect(parseDraft(draft)).toEqual({ override: check.overrides });
  expect(effectiveOf(check, draft)).toEqual({
    enabled: false,
    interval: 5,
    thresholds: { warn_percent: 70 },
    source: "hub",
  });
  expect(parseDraft({ runs: "", interval: "", thresholds: "" })).toEqual({ override: null });
  expect(parseDraft({ runs: "", interval: "0", thresholds: "" })).toHaveProperty(
    "problems.interval",
  );
  expect(
    problemsFor("disk.usage", [
      { path: "disk.usage.thresholds.warn", message: "x", severity: "error" },
    ]),
  ).toEqual({
    thresholds: "x",
  });
});
