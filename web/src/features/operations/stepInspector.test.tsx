import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";
import { at } from "../../test/at";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { operationDetail } from "../../test/operationFixtures";
import { showOperation, stepDetail } from "../../test/operationPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(resetApi);

const STEP = "GET /api/v1/operations/{id}/steps/{step}";
const detail = operationDetail("failed-verification-new-cycle");
const first = at(detail.trace.steps, 0);
const link = (id: string) => document.querySelector(`a[href$="?step=${id}"]`) as HTMLElement;
const location = () => screen.getByTestId("location");

async function ready() {
  await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
}

test("clicking_a_step_opens_the_inspector_and_puts_the_step_in_the_url", async () => {
  showOperation(detail);
  await ready();
  await userEvent.setup().click(link(first.id));
  const inspector = await screen.findByRole("complementary", { name: "Step inspector" });
  expect(location()).toHaveTextContent(`?step=${first.id}`);
  expect(inspector.contains(document.activeElement)).toBe(true);
  expect(link(first.id)).toHaveAttribute("aria-current", "true");
});

test("the_inspector_loads_the_step_and_shows_intent_and_bounded_output", async () => {
  showOperation(detail);
  await ready();
  await userEvent.setup().click(link(first.id));
  const inspector = await screen.findByRole("complementary", { name: "Step inspector" });
  expect(await within(inspector).findByText("Which container?")).toBeVisible();
  expect(within(inspector).queryByText("kind")).toBeNull();
  expect(within(inspector).getByLabelText("Step output")).toHaveTextContent("plex is down");
  expect(calls.filter((call) => call.route === STEP)).toHaveLength(1);
});

test("a_step_without_output_says_how_long_raw_outputs_are_kept", async () => {
  showOperation(detail);
  route(STEP, () => ({ ...stepDetail(detail, first.id), output: null }));
  await ready();
  await userEvent.setup().click(link(first.id));
  expect(await screen.findByText(/removed after the retention period/)).toBeVisible();
});

test("a_deep_link_opens_the_inspector_without_taking_focus", async () => {
  showOperation(detail, { search: `?step=${first.id}` });
  await screen.findByRole("complementary", { name: "Step inspector" });
  expect(document.body).toHaveFocus();
});

test("an_unknown_step_in_the_url_is_dropped", async () => {
  showOperation(detail, { search: "?step=nope" });
  await ready();
  await vi.waitFor(() => expect(location()).not.toHaveTextContent("step="));
  expect(screen.queryByRole("complementary", { name: "Step inspector" })).toBeNull();
});

test("escape_closes_the_inspector_removes_the_param_and_returns_focus_to_the_row", async () => {
  showOperation(detail);
  await ready();
  const user = userEvent.setup();
  await user.click(link(first.id));
  await screen.findByRole("complementary", { name: "Step inspector" });
  await user.keyboard("{Escape}");
  await vi.waitFor(() => expect(location()).not.toHaveTextContent("step="));
  expect(screen.queryByRole("complementary", { name: "Step inspector" })).toBeNull();
  expect(link(first.id)).toHaveFocus();
});

test("selecting_another_step_changes_content_without_moving_focus_and_announces_it", async () => {
  showOperation(detail);
  await ready();
  const user = userEvent.setup();
  await user.click(link(first.id));
  const inspector = await screen.findByRole("complementary", { name: "Step inspector" });
  const second = at(detail.trace.steps, 1);
  await user.click(link(second.id));
  expect(location()).toHaveTextContent(`?step=${second.id}`);
  expect(within(inspector).getByRole("status")).toHaveTextContent(/^Showing /);
  // The click focuses the link the person pressed; nothing moves focus into the panel.
  expect(link(second.id)).toHaveFocus();
});

test("connections_link_to_the_other_steps", async () => {
  showOperation(detail);
  await ready();
  const edge = at(detail.trace.edges, 0);
  const user = userEvent.setup();
  await user.click(link(edge.from));
  const inspector = await screen.findByRole("complementary", { name: "Step inspector" });
  const links = within(inspector).getAllByRole("link");
  await user.click(at(links, 0));
  expect(location()).toHaveTextContent(`?step=${edge.to}`);
});

test("step_detail_failure_is_a_load_error_inside_the_inspector_only", async () => {
  showOperation(detail);
  route(STEP, () => failure(503, "unavailable", "down"));
  await ready();
  await userEvent.setup().click(link(first.id));
  const inspector = await screen.findByRole("complementary", { name: "Step inspector" });
  expect(await within(inspector).findByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.getByRole("heading", { level: 1, name: detail.operation.goal })).toBeVisible();
});

test("an_execution_step_shows_the_card_read_only_and_points_at_waiting_for_you_while_pending", async () => {
  const waiting = operationDetail("waiting-approval");
  showOperation(waiting);
  await screen.findByRole("heading", { level: 1, name: waiting.operation.goal });
  await userEvent.setup().click(link(at(waiting.executions, 0).step));
  const inspector = await screen.findByRole("complementary", { name: "Step inspector" });
  expect(within(inspector).getByRole("group")).toBeVisible();
  expect(within(inspector).getByText("Decide in Waiting for you on the page.")).toBeVisible();
  expect(within(inspector).queryByRole("button", { name: "Approve" })).toBeNull();
});
