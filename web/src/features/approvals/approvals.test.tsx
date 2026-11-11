import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Action } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { action, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { ActionCard, suggestedBy } from "./ActionCard";
import { ApprovalsPage } from "./ApprovalsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

function approvals() {
  return calls.filter((call) => call.route === "POST /api/v1/actions/{id}/approve");
}

function listQueries() {
  return calls
    .filter((call) => call.route === "GET /api/v1/actions")
    .map((call) => call.init.params?.query);
}

/** Answers the waiting list with `waiting` and the decided list with `decided`. */
function routeActions(waiting: Action[], decided: Action[] | (() => Action[]) = []) {
  route("GET /api/v1/actions", ({ params }) => ({
    items: params?.query?.decided ? (typeof decided === "function" ? decided() : decided) : waiting,
    next_cursor: null,
  }));
}

/** A time `minutes` from now, as the hub writes it. */
function inMinutes(minutes: number): string {
  return new Date(Date.now() + minutes * 60_000).toISOString();
}

function open(url = "/approvals") {
  return renderAt(url, "/approvals", <ApprovalsPage />);
}

function card(skill: string) {
  return screen.findByRole("group", { name: `Action ${skill}` });
}

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("POST /api/v1/actions/{id}/approve", () => undefined);
});

test("destructive_approval_asks_first_and_requires_the_exact_skill_name", async () => {
  const user = userEvent.setup();
  routeActions([action({ id: "act-9", skill: "delete_volume", risk: "destructive" })]);
  open();

  await user.click(within(await card("delete_volume")).getByRole("button", { name: "Approve" }));
  expect(approvals(), "nothing is sent before the confirmation").toHaveLength(0);
  const dialog = screen.getByRole("dialog", { name: "Run destructive action delete_volume?" });
  expect(
    within(dialog).getByText(
      "delete_volume with name=web runs on nas as soon as you approve. It can lose data, and it cannot be undone.",
    ),
    "the consequence is in plain words",
  ).toBeVisible();
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
  expect(await screen.findByText("Approved delete_volume. It runs now.")).toBeVisible();
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("write_approval_uses_simple_confirm", async () => {
  const user = userEvent.setup();
  routeActions([action()]);
  open();

  const item = await card("restart_container");
  expect(within(item).getByText("on nas")).toBeVisible();
  await user.click(within(item).getByRole("button", { name: "Approve" }));
  const dialog = screen.getByRole("dialog", { name: "Run restart_container?" });
  expect(within(dialog).queryByRole("textbox")).toBeNull();
  expect(
    within(dialog).getByText("restart_container with name=web runs on nas as soon as you approve."),
  ).toBeVisible();

  await user.click(within(dialog).getByRole("button", { name: "Approve" }));
  expect(approvals()).toEqual([
    {
      route: "POST /api/v1/actions/{id}/approve",
      init: { params: { path: { id: "act-1" } }, body: {} },
    },
  ]);
});

test("a_refused_approval_is_explained_inside_the_dialog", async () => {
  const user = userEvent.setup();
  routeActions([action()]);
  route("POST /api/v1/actions/{id}/approve", () =>
    failure(409, "conflict", "the approval window closed 2 minutes ago"),
  );
  open();

  await user.click(
    within(await card("restart_container")).getByRole("button", { name: "Approve" }),
  );
  const dialog = within(screen.getByRole("dialog"));
  await user.click(dialog.getByRole("button", { name: "Approve" }));
  expect(await dialog.findByText("Could not approve the action")).toBeVisible();
  expect(dialog.getByText("The approval window closed 2 minutes ago.")).toBeVisible();
});

test("approvals_page_explains_that_nothing_runs_until_approved", async () => {
  routeActions([]);
  open();

  // Shows even when there is nothing to approve.
  expect(await screen.findByText(/^Actions the model suggested\./)).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about approvals (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/concepts/actions.html");
  expect(await screen.findByText("Nothing to approve")).toBeVisible();
  expect(
    screen.getByText(
      "Actions triage suggests show up here, and nothing runs until you approve it.",
    ),
  ).toBeVisible();
});

test("deny_is_one_click_and_says_nothing_ran", async () => {
  const user = userEvent.setup();
  routeActions([action()]);
  route("POST /api/v1/actions/{id}/deny", () => undefined);
  open();

  await user.click(await screen.findByRole("button", { name: "Deny" }));
  expect(calls.map((c) => c.route)).toContain("POST /api/v1/actions/{id}/deny");
  expect(approvals()).toHaveLength(0);
  expect(await screen.findByText("Denied restart_container. Nothing ran.")).toBeVisible();
});

test("waiting_actions_are_ordered_by_expiry_and_counted", async () => {
  routeActions([
    action({ id: "late", skill: "later_one", expires_at: inMinutes(50) }),
    action({ id: "soon", skill: "sooner_one", expires_at: inMinutes(20) }),
    action({ id: "mid", skill: "middle_one", expires_at: inMinutes(30) }),
  ]);
  open();

  await card("sooner_one");
  const waiting = within(screen.getByRole("region", { name: "Waiting for you" }));
  expect(
    waiting.getAllByRole("group").map((group) => group.getAttribute("aria-label")),
    "the soonest to expire is on top",
  ).toEqual(["Action sooner_one", "Action middle_one", "Action later_one"]);
  expect(screen.getByText("3 waiting")).toBeVisible();
  expect(waiting.getAllByRole("link", { name: "View incident" })[0]).toHaveAttribute(
    "href",
    "/incidents/inc-1",
  );
});

test("approvals_follow_the_host_selector", async () => {
  routeActions([]);
  open("/approvals?host=nas");

  expect(await screen.findByText(/^No action is waiting on nas\./)).toBeVisible();
  const queries = listQueries();
  expect(queries).toContainEqual({ status: "proposed", host: "nas", limit: 200 });
  expect(
    queries.every((query) => query?.host === "nas"),
    "the decided list is filtered on the hub too",
  ).toBe(true);
});

test("approvals_under_all_hosts_send_no_host_filter", async () => {
  routeActions([]);
  open();

  await screen.findByText("Nothing to approve");
  expect(listQueries()).toContainEqual({ status: "proposed", limit: 200 });
  expect(listQueries().some((query) => query && "host" in query)).toBe(false);
});

test("recently_decided_is_closed_until_opened_and_asks_for_the_last_day", async () => {
  const user = userEvent.setup();
  routeActions([], [action({ id: "old", skill: "denied_one", status: "denied" })]);
  open();

  const toggle = await screen.findByRole("button", { name: "Recently decided" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("group", { name: "Action denied_one" })).toBeNull();

  await user.click(toggle);
  const decided = await card("denied_one");
  expect(within(decided).getByText("Denied")).toBeVisible();
  expect(
    within(decided).queryByRole("button", { name: "Approve" }),
    "a decided action cannot be decided again",
  ).toBeNull();

  const asked = listQueries().find((query) => query?.decided) as { since: string };
  const hoursBack = (Date.now() - Date.parse(asked.since)) / 3_600_000;
  expect(hoursBack).toBeGreaterThan(23.9);
  expect(hoursBack).toBeLessThan(24.1);
});

test("the_result_of_an_approved_action_is_announced", async () => {
  let status = "dispatched";
  routeActions([], () => [action({ status })]);
  const { client } = open();
  await screen.findByText("Nothing to approve");

  status = "failed";
  void client.invalidateQueries({ queryKey: ["actions"] });
  expect(await screen.findByText("restart_container failed on nas")).toBeVisible();
});

test("a_result_that_was_already_in_is_not_announced", async () => {
  routeActions([], [action({ status: "succeeded" })]);
  open();

  await screen.findByText("Nothing to approve");
  expect(screen.queryByText("restart_container succeeded on nas")).toBeNull();
});

function show(shown: Action, why?: string) {
  return renderAt("/x", "/x", <ActionCard action={shown} host="nas" why={why} />);
}

test("action_card_shows_what_would_run_and_why", () => {
  show(
    action({ args: { name: "web", path: "/srv/a/very/long/path/that/needs/to/wrap" } }),
    "The container stopped answering.",
  );

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.getByText("Write")).toBeVisible();
  expect(item.getByText("Waiting for approval")).toBeVisible();
  expect(item.getByRole("button", { name: "About Write risk" })).toBeVisible();
  expect(item.getByText("name").tagName, "arguments are a list of names and values").toBe("DT");
  expect(item.getByText("/srv/a/very/long/path/that/needs/to/wrap").tagName).toBe("DD");
  expect(item.getByText("The container stopped answering.")).toBeVisible();
  expect(item.getByText(/^Suggested by triage/)).toBeVisible();
});

test("risk_is_explained_on_demand", async () => {
  const user = userEvent.setup();
  show(action({ risk: "destructive" }));

  await user.click(screen.getByRole("button", { name: "About Destructive risk" }));
  expect(
    screen.getByText("Can lose data. Needs the skill name typed to approve; never from chat."),
  ).toBeVisible();
});

test("expiry_countdown_turns_to_a_warning_near_the_end", () => {
  const far = show(action({ expires_at: inMinutes(45) }));
  const tag = () => screen.getByText(/^Expires in/);
  expect(tag()).toHaveTextContent(/Expires in 4\d:\d\d/);
  expect(tag()).not.toHaveAttribute("data-tone");
  far.unmount();

  show(action({ expires_at: inMinutes(9) }));
  expect(tag()).toHaveAttribute("data-tone", "warning");
});

test("an_expired_action_explains_itself", () => {
  show(action({ status: "expired" }));

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.getByText("Nobody decided in time, so it expired")).toBeVisible();
  expect(item.getByText("Nothing ran. Triage can propose it again.")).toBeVisible();
  expect(item.queryByRole("button", { name: "Approve" })).toBeNull();
  expect(item.queryByText(/^Expires in/)).toBeNull();
});

test("a_running_action_shows_progress_and_a_failed_one_its_reason", () => {
  const running = show(action({ status: "dispatched" }));
  expect(screen.getByRole("progressbar")).toBeVisible();
  expect(screen.getByText("Running on nas. Waiting for the result…")).toBeVisible();
  running.unmount();

  show(action({ status: "failed", reason: "exit code 1: no such container" }));
  const alert = screen.getByRole("alert");
  expect(alert).toHaveTextContent("The skill ran and reported an error; see its output.");
  expect(alert).toHaveTextContent("exit code 1: no such container");
});

test("who_suggested_an_action_reads_as_words", () => {
  expect(suggestedBy("system:triage")).toBe("Suggested by triage");
  expect(suggestedBy("system:chat")).toBe("Suggested in chat");
  expect(suggestedBy("user:0192")).toBe("Suggested by a person");
  expect(suggestedBy("system:something_new")).toBe("Suggested by Domovoi");
});
