import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Autonomy } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { AutonomyPanel } from "./AutonomyPanel";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const STATE = "GET /api/v1/policy/autonomy";
const PAUSE = "POST /api/v1/policy/autonomy/pause";
const RESUME = "POST /api/v1/policy/autonomy/resume";
const RESET = "POST /api/v1/policy/rules/{rule}/reset";

const RUNNING: Autonomy = { paused: false, suspended_rules: [] };
const PAUSED: Autonomy = {
  paused: true,
  reason: "maintenance window",
  paused_by: "owner",
  paused_at: "2026-11-25T10:00:00Z",
  suspended_rules: [{ rule: "r-restart", failures: 2, since: "2026-11-25T09:00:00Z" }],
};

beforeEach(() => {
  resetApi();
});

test("running_state_offers_pause_and_says_no_rule_is_suspended", async () => {
  route(STATE, () => RUNNING);
  renderAt("/policy", "/policy", <AutonomyPanel />);
  expect(await screen.findByRole("button", { name: "Pause" })).toBeVisible();
  expect(screen.getByText("No rule is suspended.")).toBeVisible();
  expect(screen.queryByRole("button", { name: "Resume" })).toBeNull();
});

test("pause_needs_a_reason_and_no_password", async () => {
  route(STATE, () => RUNNING);
  route(PAUSE, () => PAUSED);
  const user = userEvent.setup();
  renderAt("/policy", "/policy", <AutonomyPanel />);
  await user.click(await screen.findByRole("button", { name: "Pause" }));
  const dialog = screen.getByRole("dialog");
  expect(within(dialog).getByRole("button", { name: "Pause" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await user.type(screen.getByLabelText("Reason"), "maintenance window");
  expect(screen.queryByLabelText("Password")).toBeNull();
  await user.click(within(dialog).getByRole("button", { name: "Pause" }));
  await waitFor(() => expect(calls.some((call) => call.route === PAUSE)).toBe(true));
  expect(calls.find((call) => call.route === PAUSE)?.init.body).toEqual({
    reason: "maintenance window",
  });
  expect(await screen.findByRole("button", { name: "Resume" })).toBeVisible();
});

test("resume_asks_for_the_password_and_says_when_it_is_wrong", async () => {
  route(STATE, () => PAUSED);
  route(RESUME, () => failure(403, "forbidden", "wrong password"));
  const user = userEvent.setup();
  renderAt("/policy", "/policy", <AutonomyPanel />);
  expect(await screen.findByText("Autonomy is paused")).toBeVisible();
  expect(screen.getByText(/maintenance window/)).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Resume" }));
  await user.type(screen.getByLabelText("Password"), "nope");
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Resume" }));
  expect(await screen.findByText("That password is not right.")).toBeVisible();
});

test("a_suspended_rule_is_listed_and_reset_sends_the_password", async () => {
  route(STATE, () => PAUSED);
  route(RESET, () => RUNNING);
  const user = userEvent.setup();
  renderAt("/policy", "/policy", <AutonomyPanel />);
  expect(await screen.findByText("r-restart")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Reset" }));
  await user.type(screen.getByLabelText("Password"), "secret");
  await user.click(screen.getByRole("button", { name: "Reset rule" }));
  await waitFor(() => expect(calls.some((call) => call.route === RESET)).toBe(true));
  const call = calls.find((entry) => entry.route === RESET);
  expect(call?.init.params?.path).toEqual({ rule: "r-restart" });
  expect(call?.init.body).toEqual({ password: "secret" });
});
