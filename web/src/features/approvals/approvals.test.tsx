import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { action, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { ApprovalsPage } from "./ApprovalsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

function approvals() {
  return calls.filter((call) => call.route === "POST /api/v1/actions/{id}/approve");
}

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("POST /api/v1/actions/{id}/approve", () => undefined);
});

test("destructive_approval_requires_exact_skill_name", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/actions", () => ({
    items: [action({ id: "act-9", skill: "delete_volume", risk: "destructive" })],
    next_cursor: null,
  }));
  renderAt("/approvals", "/approvals", <ApprovalsPage />);

  await user.click(await screen.findByRole("button", { name: "Approve" }));
  const dialog = screen.getByRole("dialog", { name: "Run destructive action delete_volume?" });
  const confirm = within(dialog).getByRole("button", { name: "Approve" });
  const input = within(dialog).getByLabelText("Type delete_volume to confirm");
  expect(input).toHaveFocus();
  expect(confirm).toHaveAttribute("aria-disabled", "true");

  for (const wrong of ["Delete_volume", "delete_volume ", " delete_volume", "delete"]) {
    await user.clear(input);
    await user.type(input, wrong);
    expect(confirm, JSON.stringify(wrong)).toHaveAttribute("aria-disabled", "true");
  }
  await user.click(confirm);
  expect(approvals()).toHaveLength(0);

  await user.clear(input);
  await user.type(input, "delete_volume");
  expect(confirm).toBeEnabled();
  await user.click(confirm);
  expect(approvals()).toEqual([
    {
      route: "POST /api/v1/actions/{id}/approve",
      init: { params: { path: { id: "act-9" } }, body: { confirm_text: "delete_volume" } },
    },
  ]);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("write_approval_uses_simple_confirm", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/actions", () => ({ items: [action()], next_cursor: null }));
  renderAt("/approvals", "/approvals", <ApprovalsPage />);

  const item = await screen.findByRole("listitem");
  expect(within(item).getByText("on nas")).toBeVisible();
  await user.click(within(item).getByRole("button", { name: "Approve" }));
  const dialog = screen.getByRole("dialog", { name: "Run restart_container?" });
  expect(within(dialog).queryByRole("textbox")).toBeNull();

  await user.click(within(dialog).getByRole("button", { name: "Approve" }));
  expect(approvals()).toEqual([
    {
      route: "POST /api/v1/actions/{id}/approve",
      init: { params: { path: { id: "act-1" } }, body: {} },
    },
  ]);
  expect(calls.find((c) => c.route === "GET /api/v1/actions")?.init.params?.query).toEqual({
    status: "proposed",
    limit: 200,
  });
});

test("approvals_page_explains_that_nothing_runs_until_approved", async () => {
  route("GET /api/v1/actions", () => ({ items: [], next_cursor: null }));
  renderAt("/approvals", "/approvals", <ApprovalsPage />);

  // Shows even when there is nothing to approve.
  expect(await screen.findByText(/^Actions the model suggested\./)).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about approvals (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/concepts/actions.html");
});

test("deny_sends_no_confirmation", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/actions", () => ({ items: [action()], next_cursor: null }));
  route("POST /api/v1/actions/{id}/deny", () => undefined);
  renderAt("/approvals", "/approvals", <ApprovalsPage />);
  await user.click(await screen.findByRole("button", { name: "Deny" }));
  expect(calls.map((c) => c.route)).toContain("POST /api/v1/actions/{id}/deny");
  expect(approvals()).toHaveLength(0);
});

test("approvals_ask_the_hub_for_the_selected_hosts_actions", async () => {
  route("GET /api/v1/actions", () => ({ items: [], next_cursor: null }));
  renderAt("/approvals?host=nas", "/approvals", <ApprovalsPage />);

  expect(await screen.findByText(/^No action is waiting on nas\./)).toBeVisible();
  const asked = calls.filter((call) => call.route === "GET /api/v1/actions");
  expect(asked.map((call) => call.init.params?.query)).toEqual([
    { status: "proposed", host: "nas", limit: 200 },
  ]);
});

test("approvals_under_all_hosts_send_no_host_filter", async () => {
  route("GET /api/v1/actions", () => ({ items: [], next_cursor: null }));
  renderAt("/approvals", "/approvals", <ApprovalsPage />);

  expect(await screen.findByText("Actions triage proposes show up here.")).toBeVisible();
  const asked = calls.filter((call) => call.route === "GET /api/v1/actions");
  expect(asked[0]?.init.params?.query).toEqual({ status: "proposed", limit: 200 });
});
