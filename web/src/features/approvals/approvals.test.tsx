import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { QueryClient } from "@tanstack/react-query";
import type { Action } from "../../api/types";
import { ACTION_STATUSES } from "../../design/actionVocab";
import { ICON_NAMES } from "../../design/icons";
import { TONES } from "../../design/status";
import { actionStatusMeta } from "../../design/tone";
import { calls, resetApi, route } from "../../test/apiMock";
import { action, detail, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { ActionCard, suggestedBy } from "./ActionCard";
import { ApprovalsPage } from "./ApprovalsPage";
import { PLAN_BINDING_UNKNOWN_LABEL, PLAN_BINDING_V1_LABEL, PLAN_HASH_HINT_V2 } from "./PlanTag";

vi.mock("../../api/client", () => import("../../test/apiMock"));

function listQueries() {
  return calls
    .filter((call) => call.route === "GET /api/v1/actions")
    .map((call) => call.init.params?.query);
}

/** Answers the decided list with `decided`. */
function routeActions(decided: Action[] | (() => Action[]) = []) {
  route("GET /api/v1/actions", () => ({
    items: typeof decided === "function" ? decided() : decided,
    next_cursor: null,
  }));
}

/** Waits until the decided list has been answered, so the page has seen the first status. */
async function loaded(client: QueryClient) {
  await waitFor(() => {
    const answered = client.getQueriesData({ queryKey: ["actions"] }).some(([, data]) => data);
    expect(answered).toBe(true);
  });
}

function open(url = "/approvals") {
  return renderAt(url, "/approvals", <ApprovalsPage />);
}

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/incidents/{id}", () => detail());
});

test("approvals_page_explains_that_nothing_runs_until_approved", async () => {
  routeActions();
  open();

  // Shows even when there is nothing to approve.
  expect(await screen.findByText(/^Nothing runs until you approve it\./)).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about approvals (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/concepts/actions.html");
  expect(await screen.findByText("Approve on the operation")).toBeVisible();
  expect(screen.getByRole("link", { name: "operations" })).toHaveAttribute("href", "/operations");
  expect(screen.queryByRole("button", { name: "Approve" }), "no action can be approved").toBeNull();
});

test("approvals_follow_the_host_selector", async () => {
  routeActions();
  open("/approvals?host=nas");

  await screen.findByText("Approve on the operation");
  const queries = listQueries();
  expect(queries.length).toBeGreaterThan(0);
  expect(
    queries.every((query) => query?.host === "nas"),
    "the decided list is filtered on the hub",
  ).toBe(true);
  expect(
    queries.some((query) => query?.status === "proposed"),
    "nothing asks for waiting actions",
  ).toBe(false);
});

test("approvals_under_all_hosts_send_no_host_filter", async () => {
  routeActions();
  open();

  await screen.findByText("Approve on the operation");
  expect(listQueries().some((query) => query && "host" in query)).toBe(false);
});

test("recently_decided_is_a_compact_list_closed_until_opened_and_asks_for_the_last_day", async () => {
  const user = userEvent.setup();
  routeActions([
    action({ id: "old", skill: "denied_one", status: "denied", approved_by: "user:7" }),
    action({ id: "gone", skill: "expired_one", status: "expired" }),
  ]);
  open();

  const toggle = await screen.findByRole("button", { name: "Recently decided" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("denied_one")).toBeNull();

  await user.click(toggle);
  const table = await screen.findByRole("table", { name: "Recently decided" });
  const rows = within(table).getAllByRole("row");
  const cells = (row: number) =>
    within(rows[row] as HTMLElement)
      .getAllByRole("cell")
      .slice(0, 4)
      .map((cell) => cell.textContent);
  expect(cells(1), "what, decision, where, who").toEqual([
    "denied_one",
    "Denied",
    "nas",
    "A person",
  ]);
  expect(cells(2)).toEqual(["expired_one", "Expired", "nas", "Nobody, it expired"]);
  expect(
    within(table).queryByRole("button", { name: "Approve" }),
    "a decided action cannot be decided again",
  ).toBeNull();
  expect(within(table).getByRole("link", { name: "denied_one" })).toHaveAttribute(
    "href",
    "/incidents/inc-1",
  );
  await user.click(within(rows[1] as HTMLElement).getByText("nas"));
  expect(screen.getByTestId("location"), "the whole row opens the incident").toHaveTextContent(
    "/incidents/inc-1",
  );

  const asked = listQueries().find((query) => query?.decided) as { since: string };
  const hoursBack = (Date.now() - Date.parse(asked.since)) / 3_600_000;
  expect(hoursBack).toBeGreaterThan(23.9);
  expect(hoursBack).toBeLessThan(24.1);
});

test("the_result_of_an_approved_action_is_announced", async () => {
  let status = "dispatched";
  routeActions(() => [action({ status })]);
  const { client } = open();
  await loaded(client);

  status = "failed";
  void client.invalidateQueries({ queryKey: ["actions"] });
  expect(await screen.findByText("restart_container failed on nas")).toBeVisible();
});

test("a_result_that_was_already_in_is_not_announced", async () => {
  routeActions([action({ status: "succeeded" })]);
  const { client } = open();

  await loaded(client);
  expect(screen.queryByText("restart_container succeeded on nas")).toBeNull();
});

test("an_unknown_result_is_information_and_not_a_failure", async () => {
  let status = "dispatched";
  routeActions(() => [action({ status, reason: "the agent lost its record of this run" })]);
  const { client } = open();
  await loaded(client);

  status = "outcome_unknown";
  void client.invalidateQueries({ queryKey: ["actions"] });

  const title = await screen.findByText("restart_container result is unknown on nas");
  expect(title).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByText("the agent lost its record of this run")).toBeVisible();
});

test("a_run_that_never_happened_is_information_and_not_a_success", async () => {
  let status = "dispatched";
  routeActions(() => [action({ status })]);
  const { client } = open();
  await loaded(client);

  status = "not_run";
  void client.invalidateQueries({ queryKey: ["actions"] });

  expect(await screen.findByText("restart_container never ran on nas")).toBeVisible();
  expect(screen.queryByText("restart_container succeeded on nas")).toBeNull();
});

test("a_failure_is_announced_as_an_error_and_a_success_as_a_success", async () => {
  let status = "dispatched";
  routeActions(() => [action({ status, reason: "exit code 1" })]);
  const { client } = open();
  await loaded(client);

  status = "failed";
  void client.invalidateQueries({ queryKey: ["actions"] });
  expect(await screen.findByText("restart_container failed on nas")).toBeVisible();
  expect(screen.getByRole("alert")).toBeVisible();
});

test("a_success_is_announced_as_a_success", async () => {
  let status = "dispatched";
  routeActions(() => [action({ status })]);
  const { client } = open();
  await loaded(client);

  status = "succeeded";
  void client.invalidateQueries({ queryKey: ["actions"] });
  expect(await screen.findByText("restart_container succeeded on nas")).toBeVisible();
});

function show(shown: Action, why?: string) {
  return renderAt("/x", "/x", <ActionCard action={shown} host="nas" why={why} />);
}

test("action_card_shows_what_ran_and_the_models_reason", () => {
  show(
    action({
      status: "failed",
      reason: "exit code 1",
      args: { name: "web", path: "/srv/a/very/long/path/that/needs/to/wrap" },
    }),
    "The container stopped answering.",
  );

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.getByText("Write")).toBeVisible();
  expect(item.getByText("Legacy"), "the card says it is history").toBeVisible();
  expect(item.queryByRole("button", { name: "Approve" }), "history cannot be decided").toBeNull();
  expect(item.queryByRole("button", { name: "Deny" })).toBeNull();
  expect(item.getByRole("button", { name: "About Write risk" })).toBeVisible();
  expect(item.getByText("name = web").tagName, "arguments are key = value lines").toBe("LI");
  expect(item.getByText("path = /srv/a/very/long/path/that/needs/to/wrap")).toBeVisible();
  expect(item.getByText("The container stopped answering.")).toBeVisible();
  expect(item.getByText("Model's reason (not checked):")).toBeVisible();
  expect(item.getByText(/^Suggested by triage/)).toBeVisible();
});

test("action_card_shows_the_plan_block", async () => {
  const user = userEvent.setup();
  const shown = action({ status: "failed", args: { name: "web", path: "/srv" } });
  show(shown);

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.getByText("on nas")).toBeVisible();
  expect(item.getByText("name = web")).toBeVisible();
  expect(item.getByText("path = /srv"), "every argument is shown").toBeVisible();
  expect(item.getByText("Write")).toBeVisible();
  expect(item.queryByText(/^Expires in/), "history has no countdown").toBeNull();
  expect(item.getByText(`Plan ${shown.plan_hash.slice(0, 8)}`)).toBeVisible();
  expect(item.getByText("v2")).toBeVisible();

  await user.click(item.getByRole("button", { name: "About Plan hash v2" }));
  expect(screen.getByText(PLAN_HASH_HINT_V2)).toBeVisible();
});

test("action_card_marks_legacy_actions_as_v1_binding", () => {
  show(action({ id: "act-v1", status: "failed", plan_binding: "v1", plan_hash_v1: null }));

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.getByText(PLAN_BINDING_V1_LABEL)).toBeVisible();
  expect(item.queryByText("v2")).toBeNull();
});

test("unknown_plan_binding_is_never_shown_as_v1", () => {
  show(action({ id: "act-odd", status: "failed", plan_binding: "v3" as never }));

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.getByText(PLAN_BINDING_UNKNOWN_LABEL)).toBeVisible();
  expect(item.queryByText(PLAN_BINDING_V1_LABEL)).toBeNull();
});

test("model_reason_is_labelled_as_the_models", () => {
  show(action({ status: "failed" }), "The container stopped answering.");

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.getByText("Model's reason (not checked):")).toBeVisible();
  expect(item.queryByText("Why:")).toBeNull();
});

test("risk_is_explained_on_demand", async () => {
  const user = userEvent.setup();
  show(action({ status: "failed", risk: "destructive" }));

  await user.click(screen.getByRole("button", { name: "About Destructive risk" }));
  expect(
    screen.getByText("Can lose data. Needs the skill name typed to approve; never from chat."),
  ).toBeVisible();
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

test("outcome_unknown_is_not_in_flight", () => {
  show(
    action({
      status: "outcome_unknown",
      reason: "The agent stopped answering before it reported back.",
    }),
  );

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.queryByRole("progressbar"), "no progress spinner").toBeNull();
  expect(item.queryByText(/Waiting for the result/)).toBeNull();
  expect(item.getByText("Result unknown")).toBeVisible();
  expect(
    item.getByText(
      "It was sent to the host and may have run; check the host before running it again.",
    ),
  ).toBeVisible();
  expect(item.getByText("The agent stopped answering before it reported back.")).toBeVisible();
});

test("outcome_unknown_and_not_run_have_vocabulary", () => {
  for (const status of ["outcome_unknown", "not_run"] as const) {
    expect(ACTION_STATUSES[status], "the status is in the vocabulary").toBeDefined();
    expect(actionStatusMeta(status).label).not.toBe("");
    expect(TONES).toContain(actionStatusMeta(status).tone);
    expect(actionStatusMeta(status).hint).not.toBe("");
    expect(ICON_NAMES).toContain(actionStatusMeta(status).icon);
  }
  expect(actionStatusMeta("outcome_unknown").tone, "an unknown result is a warning").toBe(
    "warning",
  );
  expect(actionStatusMeta("not_run").tone, "nothing was attempted is quiet").toBe("neutral");
});

test("who_suggested_an_action_reads_as_words", () => {
  expect(suggestedBy("system:triage")).toBe("Suggested by triage");
  expect(suggestedBy("system:chat")).toBe("Suggested in chat");
  expect(suggestedBy("user:0192")).toBe("Suggested by a person");
  expect(suggestedBy("system:something_new")).toBe("Suggested by Domovoi");
});
