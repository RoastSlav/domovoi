import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Action } from "../../api/types";
import { ACTION_STATUSES } from "../../design/actionVocab";
import { ICON_NAMES } from "../../design/icons";
import { TONES } from "../../design/status";
import { actionStatusMeta } from "../../design/tone";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { action, detail, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { ActionCard, suggestedBy } from "./ActionCard";
import { ApprovalsPage } from "./ApprovalsPage";
import { PLAN_BINDING_UNKNOWN_LABEL, PLAN_BINDING_V1_LABEL, PLAN_HASH_HINT_V2 } from "./PlanTag";

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
  route("GET /api/v1/incidents/{id}", () => detail());
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
      init: {
        params: { path: { id: "act-9" } },
        body: { plan_hash: action().plan_hash, confirm_text: "delete_volume" },
      },
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
      init: { params: { path: { id: "act-1" } }, body: { plan_hash: action().plan_hash } },
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
  expect(
    await screen.findByText(/^Actions the model suggested; nothing runs until you approve them\./),
  ).toBeVisible();
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
  expect(
    (await waiting.findAllByRole("link", { name: "Disk /srv is 93% full" }))[0],
  ).toHaveAttribute("href", "/incidents/inc-1");
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

test("recently_decided_is_a_compact_list_closed_until_opened_and_asks_for_the_last_day", async () => {
  const user = userEvent.setup();
  routeActions(
    [],
    [
      action({ id: "old", skill: "denied_one", status: "denied", approved_by: "user:7" }),
      action({ id: "gone", skill: "expired_one", status: "expired" }),
    ],
  );
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

test("an_unknown_result_is_information_and_not_a_failure", async () => {
  let status = "dispatched";
  routeActions([], () => [action({ status, reason: "the agent lost its record of this run" })]);
  const { client } = open();
  await screen.findByText("Nothing to approve");

  status = "outcome_unknown";
  void client.invalidateQueries({ queryKey: ["actions"] });

  const title = await screen.findByText("restart_container result is unknown on nas");
  expect(title).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByText("the agent lost its record of this run")).toBeVisible();
});

test("a_run_that_never_happened_is_information_and_not_a_success", async () => {
  let status = "dispatched";
  routeActions([], () => [action({ status })]);
  const { client } = open();
  await screen.findByText("Nothing to approve");

  status = "not_run";
  void client.invalidateQueries({ queryKey: ["actions"] });

  expect(await screen.findByText("restart_container never ran on nas")).toBeVisible();
  expect(screen.queryByText("restart_container succeeded on nas")).toBeNull();
});

test("a_failure_is_announced_as_an_error_and_a_success_as_a_success", async () => {
  let status = "dispatched";
  routeActions([], () => [action({ status, reason: "exit code 1" })]);
  const { client } = open();
  await screen.findByText("Nothing to approve");

  status = "failed";
  void client.invalidateQueries({ queryKey: ["actions"] });
  expect(await screen.findByText("restart_container failed on nas")).toBeVisible();
  expect(screen.getByRole("alert")).toBeVisible();
});

test("a_success_is_announced_as_a_success", async () => {
  let status = "dispatched";
  routeActions([], () => [action({ status })]);
  const { client } = open();
  await screen.findByText("Nothing to approve");

  status = "succeeded";
  void client.invalidateQueries({ queryKey: ["actions"] });
  expect(await screen.findByText("restart_container succeeded on nas")).toBeVisible();
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
  expect(item.queryByText("Waiting for approval"), "the section already says it").toBeNull();
  expect(item.getByRole("button", { name: "About Write risk" })).toBeVisible();
  expect(item.getByText("name = web").tagName, "arguments are key = value lines").toBe("LI");
  expect(item.getByText("path = /srv/a/very/long/path/that/needs/to/wrap")).toBeVisible();
  expect(item.getByText("The container stopped answering.")).toBeVisible();
  expect(item.getByText("Model's reason (not checked):")).toBeVisible();
  expect(item.getByText(/^Suggested by triage/)).toBeVisible();
});

test("approve_request_carries_the_plan_hash", async () => {
  const user = userEvent.setup();
  const write = action({ id: "act-w" });
  const wipe = action({ id: "act-d", skill: "delete_volume", risk: "destructive" });
  routeActions([write, wipe]);
  open();

  await user.click(
    within(await card("restart_container")).getByRole("button", { name: "Approve" }),
  );
  await user.click(
    within(screen.getByRole("dialog", { name: "Run restart_container?" })).getByRole("button", {
      name: "Approve",
    }),
  );

  await user.click(within(await card("delete_volume")).getByRole("button", { name: "Approve" }));
  const dialog = within(screen.getByRole("dialog"));
  await user.type(dialog.getByLabelText("Type delete_volume to confirm"), "delete_volume");
  await user.click(dialog.getByRole("button", { name: "Approve" }));

  expect(approvals()).toEqual([
    {
      route: "POST /api/v1/actions/{id}/approve",
      init: { params: { path: { id: "act-w" } }, body: { plan_hash: write.plan_hash } },
    },
    {
      route: "POST /api/v1/actions/{id}/approve",
      init: {
        params: { path: { id: "act-d" } },
        body: { plan_hash: wipe.plan_hash, confirm_text: "delete_volume" },
      },
    },
  ]);
});

test("waiting_action_card_shows_the_plan_block", async () => {
  const user = userEvent.setup();
  const shown = action({ args: { name: "web", path: "/srv" } });
  show(shown);

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.getByText("on nas")).toBeVisible();
  expect(item.getByText("name = web")).toBeVisible();
  expect(item.getByText("path = /srv"), "every argument is shown").toBeVisible();
  expect(item.getByText("Write")).toBeVisible();
  expect(item.getByText(/^Expires in/)).toBeVisible();
  expect(item.getByText(`Plan ${shown.plan_hash.slice(0, 8)}`)).toBeVisible();
  expect(item.getByText("v2")).toBeVisible();

  await user.click(item.getByRole("button", { name: "About Plan hash v2" }));
  expect(screen.getByText(PLAN_HASH_HINT_V2)).toBeVisible();
});

test("action_card_marks_legacy_actions_as_v1_binding", async () => {
  const legacy = action({ id: "act-v1", plan_binding: "v1", plan_hash_v1: null });
  routeActions([legacy]);
  open();

  const item = within(await card("restart_container"));
  expect(item.getByText(PLAN_BINDING_V1_LABEL)).toBeVisible();
  expect(item.queryByText("v2")).toBeNull();
});

test("unknown_plan_binding_is_never_shown_as_v1", async () => {
  const odd = action({ id: "act-odd", plan_binding: "v3" as never });
  routeActions([odd]);
  open();

  const item = within(await card("restart_container"));
  expect(item.getByText(PLAN_BINDING_UNKNOWN_LABEL)).toBeVisible();
  expect(item.queryByText(PLAN_BINDING_V1_LABEL)).toBeNull();
});

test("model_reason_is_labelled_as_the_models", () => {
  show(action(), "The container stopped answering.");

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.getByText("Model's reason (not checked):")).toBeVisible();
  expect(item.queryByText("Why:")).toBeNull();
});

test("stale_plan_refusal_is_explained_in_the_dialog", async () => {
  const user = userEvent.setup();
  routeActions([action()]);
  route("POST /api/v1/actions/{id}/approve", () =>
    failure(
      409,
      "conflict",
      "the plan changed since you opened it; reload the page and review it again",
    ),
  );
  open();

  await user.click(
    within(await card("restart_container")).getByRole("button", { name: "Approve" }),
  );
  const dialog = within(screen.getByRole("dialog"));
  await user.click(dialog.getByRole("button", { name: "Approve" }));
  expect(await dialog.findByText("Could not approve the action")).toBeVisible();
  expect(
    dialog.getByText("The plan changed since you opened it; reload the page and review it again."),
  ).toBeVisible();
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

test("a_waiting_card_shows_why_and_names_the_incident_it_is_for", async () => {
  routeActions([action()]);
  route("GET /api/v1/incidents/{id}", () => ({
    ...detail(),
    report: {
      headline: "",
      summary: "",
      cause: "",
      evidence: [],
      verdict: "ACTION_NEEDED",
      next_step: "",
      manual_steps: [],
      proposals: [{ skill: "restart_container", args: { name: "web" }, reason: "It is stuck." }],
      details: "",
    },
  }));
  open();

  const item = within(await card("restart_container"));
  expect(await item.findByText("It is stuck.")).toBeVisible();
  expect(item.getByRole("link", { name: "Disk /srv is 93% full" })).toHaveAttribute(
    "href",
    "/incidents/inc-1",
  );
  expect(item.getByText("name = web"), "the arguments are shown before approving").toBeVisible();
});

test("a_waiting_card_still_works_when_the_incident_cannot_be_read", async () => {
  routeActions([action()]);
  route("GET /api/v1/incidents/{id}", () => failure(500, "internal", "the hub is sick"));
  open();

  const item = within(await card("restart_container"));
  expect(item.getByRole("link", { name: "View incident" })).toBeVisible();
  expect(item.getByRole("button", { name: "Approve" })).toBeVisible();
});

describe("approve emphasis", () => {
  const approve = () => screen.getByRole("button", { name: "Approve" });

  test("approve_is_the_cards_primary_on_the_approvals_page", () => {
    renderAt("/x", "/x", <ActionCard action={action()} host="nas" />);
    expect(approve().className).toMatch(/primary/);
  });

  test("approve_steps_back_to_secondary_where_the_page_has_its_own_primary", () => {
    renderAt("/x", "/x", <ActionCard action={action()} host="nas" approve="secondary" />);
    expect(approve().className, "the accent icon marks the quiet Approve").toMatch(/approveIcon/);
    expect(approve().className).not.toMatch(/primary/);
  });

  test("a_destructive_approve_is_a_danger_button_whatever_the_page_says", () => {
    renderAt(
      "/x",
      "/x",
      <ActionCard
        action={action({ risk: "destructive", skill: "delete_volume" })}
        host="nas"
        approve="secondary"
      />,
    );
    expect(approve().className).toMatch(/danger/);
  });
});

/** Five waiting actions, the last one destructive, expiring at different times. */
function five(): Action[] {
  return [
    action({ id: "a1", skill: "skill_e", expires_at: inMinutes(50) }),
    action({ id: "a2", skill: "skill_a", expires_at: inMinutes(10) }),
    action({ id: "a3", skill: "skill_c", expires_at: inMinutes(30) }),
    action({ id: "a4", skill: "skill_b", expires_at: inMinutes(20) }),
    action({
      id: "a5",
      skill: "delete_volume",
      risk: "destructive",
      expires_at: inMinutes(40),
      args: { name: "old", path: "/srv/very/long" },
    }),
  ];
}

test("more_than_three_waiting_actions_are_compact_rows_soonest_first", async () => {
  routeActions(five());
  open();

  const list = await screen.findByRole("list", { name: "Waiting actions" });
  const rows = within(list).getAllByRole("listitem");
  expect(rows).toHaveLength(5);
  expect(rows[0], "the soonest to expire is on top").toHaveTextContent("skill_a");
  expect(rows[1]).toHaveTextContent("skill_b");
  expect(rows[0]).toHaveTextContent("Write");
  expect(rows[0]).toHaveTextContent("nas");
  expect(rows[0]).toHaveTextContent("name=web");
  expect(rows[0]).toHaveTextContent(/Expires in/);
  expect(within(rows[0] as HTMLElement).getByRole("button", { name: "Approve" })).toBeVisible();
  expect(within(rows[0] as HTMLElement).getByRole("button", { name: "Deny" })).toBeVisible();
  expect(
    screen.queryByRole("group", { name: /^Action / }),
    "no full cards until a row is opened",
  ).toBeNull();
});

test("three_or_fewer_waiting_actions_stay_full_cards", async () => {
  routeActions(five().slice(0, 3));
  open();

  await card("skill_e");
  expect(screen.queryByRole("list", { name: "Waiting actions" })).toBeNull();
});

test("a_compact_row_opens_in_place_to_the_full_card_and_folds_back", async () => {
  const user = userEvent.setup();
  routeActions(five());
  open();

  await user.click(await screen.findByRole("button", { name: "Show details of skill_a" }));
  const item = within(await card("skill_a"));
  expect(item.getByText("name = web")).toBeVisible();
  expect(item.getByRole("link", { name: "Disk /srv is 93% full" })).toBeVisible();
  expect(
    screen.getAllByRole("button", { name: /^Show details of/ }),
    "the other four stay rows",
  ).toHaveLength(4);

  await user.click(screen.getByRole("button", { name: "Show less" }));
  expect(screen.queryByRole("group", { name: "Action skill_a" })).toBeNull();
  expect(screen.getByRole("button", { name: "Show details of skill_a" })).toBeVisible();
});

test("a_destructive_action_still_asks_in_its_dialog_from_a_compact_row", async () => {
  const user = userEvent.setup();
  routeActions(five());
  open();

  const row = (await screen.findByText("delete_volume")).closest("li") as HTMLElement;
  const approve = within(row).getByRole("button", { name: "Approve" });
  expect(approve.className).toMatch(/danger/);
  await user.click(approve);
  expect(approvals(), "nothing is sent before the confirmation").toHaveLength(0);
  expect(
    screen.getByRole("dialog", { name: "Run destructive action delete_volume?" }),
  ).toBeVisible();
});

test("an_expired_but_unswept_action_says_expired_and_cannot_be_approved", async () => {
  const user = userEvent.setup();
  renderAt("/x", "/x", <ActionCard action={action({ expires_at: inMinutes(-5) })} host="nas" />);

  const item = within(screen.getByRole("group", { name: "Action restart_container" }));
  expect(item.getByText("Expired")).toBeVisible();
  expect(item.queryByText("Expiring now")).toBeNull();
  const approve = item.getByRole("button", { name: "Approve" });
  expect(approve).toHaveAttribute("aria-disabled", "true");
  expect(approve).toHaveAccessibleDescription("This request expired; triage can propose it again.");
  await user.click(approve);
  expect(screen.queryByRole("dialog"), "no confirmation for a shut window").toBeNull();
  expect(item.getByRole("button", { name: "Deny" }), "denying it is still possible").toBeVisible();
});

test("an_expired_waiting_row_cannot_be_approved_either", async () => {
  routeActions([
    ...five().slice(0, 3),
    action({ id: "old", skill: "gone_one", expires_at: inMinutes(-2) }),
  ]);
  open();

  const row = (await screen.findByText("gone_one")).closest("li") as HTMLElement;
  expect(within(row).getByText("Expired")).toBeVisible();
  expect(within(row).getByRole("button", { name: "Approve" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
});
