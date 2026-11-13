import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AckRule, AckRuleInput, Host } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { RulesPage } from "./RulesPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const HOSTS = "GET /api/v1/hosts";
const RULES = "GET /api/v1/ack-rules";
const CREATE = "POST /api/v1/ack-rules";
const DELETE = "DELETE /api/v1/ack-rules/{id}";

/** A time a few days from `now`, for the table to show as not yet expired. */
function inDays(days: number): string {
  return new Date(Date.now() + days * 86_400_000).toISOString();
}

const NAS: Host = {
  id: "0196ac01-0000-7000-8000-000000000100",
  name: "nas",
  kind: "agent",
  connected: true,
  agent_enrolled: true,
  open_incidents: 0,
  outdated: false,
  protocol_state: "compatible",
  reported_protocol: "1.1",
  hub_protocol: "1.1",
  reported_release: "0.0.1",
  incompatible_since: null,
};

const BACKUP: AckRule = {
  id: "0196ac01-0000-7000-8000-000000000200",
  check: "disk.usage",
  host: "nas",
  target_pattern: "/srv",
  predicates: [{ key: "used_percent", op: "at_most", value: "95" }],
  note: "known backup window",
  expires_at: inDays(3),
};

const OLD: AckRule = {
  id: "0196ac01-0000-7000-8000-000000000201",
  check: "security.ssh_login",
  host: null,
  target_pattern: null,
  predicates: [],
  note: "expected brute force",
  expires_at: inDays(-2),
};

const FOREVER: AckRule = {
  id: "0196ac01-0000-7000-8000-000000000202",
  check: "memory.available",
  host: "pi",
  target_pattern: null,
  predicates: [],
  note: "this host is small",
  expires_at: null,
};

let stored: AckRule[];

beforeEach(() => {
  resetApi();
  stored = [BACKUP, OLD];
  route(HOSTS, () => [NAS]);
  route(RULES, () => stored);
  route(CREATE, (init) => {
    const body = init.body as AckRuleInput;
    const created: AckRule = {
      id: "0196ac01-0000-7000-8000-000000000203",
      check: body.check,
      host: body.host ?? null,
      target_pattern: body.target_pattern ?? null,
      predicates: body.predicates,
      note: body.note,
      expires_at: inDays(1),
    };
    stored = [...stored, created];
    return created;
  });
  route(DELETE, (init) => {
    stored = stored.filter((rule) => rule.id !== init.params?.path?.id);
    return null;
  });
});

type User = ReturnType<typeof userEvent.setup>;

/** The rows of the table, top to bottom. */
function rowOf(text: string | RegExp): HTMLElement {
  const table = screen.getByRole("table", { name: "Known conditions" });
  return within(table).getByText(text).closest("tr") as HTMLElement;
}

/** Opens the add dialog and returns it. */
async function openAdd(user: User): Promise<HTMLElement> {
  await user.click(
    screen.getAllByRole("button", { name: "Add known condition" })[0] as HTMLElement,
  );
  return screen.findByRole("dialog", { name: "Add a known condition" });
}

/** Adds one condition row to the dialog. */
async function addRow(user: User, dialog: HTMLElement) {
  await user.click(within(dialog).getByRole("button", { name: "Add condition" }));
}

test("lists_each_known_condition_as_a_readable_phrase", async () => {
  stored = [BACKUP, OLD, FOREVER];
  renderAt("/rules", "/rules", <RulesPage />);

  const backup = await screen.findByText("Disk usage on /srv, when used percent is at most 95");
  expect(backup).toBeVisible();
  expect(
    within(rowOf("Disk usage on /srv, when used percent is at most 95")).getByText("nas"),
  ).toBeVisible();
  expect(screen.getByText("known backup window")).toBeVisible();
  expect(screen.getByText("in 3 days")).toBeVisible();
  expect(screen.getByText("Security ssh login")).toBeVisible();
  expect(within(rowOf("Security ssh login")).getByText("Any host")).toBeVisible();
  expect(within(rowOf("Security ssh login")).getByText("Expired")).toBeVisible();
  expect(within(rowOf("Memory available")).getByText("Always")).toBeVisible();
});

test("the_page_explains_what_a_known_condition_does", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);

  expect(await screen.findByRole("heading", { level: 1, name: "Known conditions" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "About Known conditions" }));
  expect(
    screen.getByText(/^Incidents that match are kept in the history and never notify\./),
  ).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about known conditions (opens in a new tab)" }),
  ).toHaveAttribute(
    "href",
    "https://RoastSlav.github.io/domovoi/concepts/incidents.html#known-conditions",
  );
});

test("the_preview_sentence_matches_the_predicates", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");
  const dialog = await openAdd(user);
  expect(within(dialog).getByText("Type the check to see the rule described here.")).toBeVisible();

  await user.type(within(dialog).getByRole("textbox", { name: /^Check/ }), "disk.usage");
  await user.type(within(dialog).getByRole("textbox", { name: "Target" }), "/srv");
  await user.selectOptions(await within(dialog).findByRole("combobox", { name: "Host" }), "nas");
  await addRow(user, dialog);
  await user.type(
    within(dialog).getByRole("textbox", { name: "Key of condition 1" }),
    "used_percent",
  );
  await user.selectOptions(
    within(dialog).getByRole("combobox", { name: "Compare of condition 1" }),
    "at_most",
  );
  await user.type(within(dialog).getByRole("spinbutton", { name: "Value of condition 1" }), "95");

  expect(
    within(dialog).getByText(
      "On nas: Disk usage on /srv, when used percent is at most 95. Matching incidents are kept in the history and never notify, for 1 hour.",
    ),
  ).toBeVisible();

  await addRow(user, dialog);
  await user.type(within(dialog).getByRole("textbox", { name: "Key of condition 2" }), "disk_name");
  await user.type(within(dialog).getByRole("textbox", { name: "Value of condition 2" }), "sda");
  await user.selectOptions(within(dialog).getByRole("combobox", { name: "Lasts" }), "");
  expect(
    within(dialog).getByText(
      "On nas: Disk usage on /srv, when used percent is at most 95 and disk name is sda. Matching incidents are kept in the history and never notify, until you delete the rule.",
    ),
  ).toBeVisible();
});

test("the_value_input_fits_the_operator", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");
  const dialog = await openAdd(user);
  await addRow(user, dialog);

  expect(within(dialog).getByRole("textbox", { name: "Value of condition 1" })).toBeVisible();
  const op = within(dialog).getByRole("combobox", { name: "Compare of condition 1" });
  await user.selectOptions(op, "at_least");
  expect(within(dialog).getByRole("spinbutton", { name: "Value of condition 1" })).toBeVisible();
  await user.selectOptions(op, "in_network");
  expect(within(dialog).getByRole("textbox", { name: "Value of condition 1" })).toBeVisible();
  expect(within(dialog).getByText(/CIDR form, for example 192\.168\.1\.0\/24/)).toBeVisible();
});

test("adds_a_known_condition_from_the_dialog", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");
  const dialog = await openAdd(user);

  await user.type(within(dialog).getByRole("textbox", { name: /^Check/ }), "ssh_guard");
  await user.selectOptions(await within(dialog).findByRole("combobox", { name: "Host" }), "nas");
  await user.type(within(dialog).getByRole("textbox", { name: "Target" }), "/var/log/auth.log");
  await user.type(within(dialog).getByRole("textbox", { name: /^Note/ }), "known brute force");
  await user.selectOptions(within(dialog).getByRole("combobox", { name: "Lasts" }), "1440");
  await addRow(user, dialog);
  await user.type(within(dialog).getByRole("textbox", { name: "Key of condition 1" }), "ip");
  await user.selectOptions(
    within(dialog).getByRole("combobox", { name: "Compare of condition 1" }),
    "in_network",
  );
  await user.type(
    within(dialog).getByRole("textbox", { name: "Value of condition 1" }),
    "10.0.0.0/8",
  );
  await user.click(within(dialog).getByRole("button", { name: "Add known condition" }));

  expect(await screen.findByText("Known condition added")).toBeVisible();
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(await screen.findByText(/^Ssh guard on \/var\/log\/auth\.log/)).toBeVisible();
  expect(calls.find((call) => call.route === CREATE)?.init.body).toEqual({
    check: "ssh_guard",
    host: "nas",
    target_pattern: "/var/log/auth.log",
    note: "known brute force",
    expires_in_minutes: 1440,
    predicates: [{ key: "ip", op: "in_network", value: "10.0.0.0/8" }],
  });
});

test("the_thirty_day_option_is_thirty_days", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");
  const dialog = await openAdd(user);

  await user.type(within(dialog).getByRole("textbox", { name: /^Check/ }), "backup.nightly");
  await user.type(within(dialog).getByRole("textbox", { name: /^Note/ }), "a monthly note");
  await user.selectOptions(within(dialog).getByRole("combobox", { name: "Lasts" }), "43200");
  await user.click(within(dialog).getByRole("button", { name: "Add known condition" }));

  await screen.findByText("Known condition added");
  expect(calls.find((call) => call.route === CREATE)?.init.body).toMatchObject({
    expires_in_minutes: 43200,
    host: null,
  });
});

test("removing_a_condition_row_leaves_it_out_of_the_body", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");
  const dialog = await openAdd(user);

  await user.type(within(dialog).getByRole("textbox", { name: /^Check/ }), "disk.usage");
  await user.type(within(dialog).getByRole("textbox", { name: /^Note/ }), "no conditions");
  await addRow(user, dialog);
  await user.type(within(dialog).getByRole("textbox", { name: "Key of condition 1" }), "disk_name");
  await user.click(within(dialog).getByRole("button", { name: "Remove condition 1" }));
  expect(within(dialog).queryByRole("textbox", { name: "Key of condition 1" })).toBeNull();
  await user.click(within(dialog).getByRole("button", { name: "Add known condition" }));

  await screen.findByText("Known condition added");
  expect(calls.find((call) => call.route === CREATE)?.init.body).toMatchObject({ predicates: [] });
});

test("a_mistake_marks_its_field_and_row_and_sends_nothing", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");
  const dialog = await openAdd(user);

  await addRow(user, dialog);
  await user.selectOptions(
    within(dialog).getByRole("combobox", { name: "Compare of condition 1" }),
    "at_most",
  );
  await user.click(within(dialog).getByRole("button", { name: "Add known condition" }));

  expect(within(dialog).getByRole("textbox", { name: /^Check/ })).toBeInvalid();
  expect(within(dialog).getByRole("textbox", { name: /^Note/ })).toBeInvalid();
  expect(within(dialog).getByRole("textbox", { name: "Key of condition 1" })).toBeInvalid();
  expect(within(dialog).getByRole("spinbutton", { name: "Value of condition 1" })).toBeInvalid();
  expect(within(dialog).getByText("Type the value to compare with.")).toBeVisible();
  expect(calls.some((call) => call.route === CREATE)).toBe(false);
});

test("a_hub_issue_marks_the_condition_row_it_is_about", async () => {
  route(CREATE, () =>
    failure(400, "invalid", "invalid rule", undefined, [
      { path: "predicates[0].value", message: "not a network", severity: "error" },
      { path: "predicates", message: "too many", severity: "error" },
    ]),
  );
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");
  const dialog = await openAdd(user);

  await user.type(within(dialog).getByRole("textbox", { name: /^Check/ }), "disk.usage");
  await user.type(within(dialog).getByRole("textbox", { name: /^Note/ }), "a note");
  await addRow(user, dialog);
  await user.type(within(dialog).getByRole("textbox", { name: "Key of condition 1" }), "ip");
  await user.type(within(dialog).getByRole("textbox", { name: "Value of condition 1" }), "x");
  await user.click(within(dialog).getByRole("button", { name: "Add known condition" }));

  expect(await within(dialog).findByText("not a network")).toBeVisible();
  expect(within(dialog).getByRole("textbox", { name: "Value of condition 1" })).toBeInvalid();
  expect(within(dialog).getByRole("textbox", { name: "Key of condition 1" })).toBeValid();
  expect(within(dialog).getByText("Could not add the known condition")).toBeVisible();
  expect(within(dialog).getByText(/too many/)).toBeVisible();
  expect(screen.getByRole("dialog")).toBeVisible();
});

test("a_host_list_error_shows_retry_instead_of_an_empty_list", async () => {
  let fail = true;
  route(HOSTS, () => (fail ? failure(500, "internal", "The hub is busy.") : [NAS]));
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");
  const dialog = await openAdd(user);

  expect(await within(dialog).findByText("Could not load hosts")).toBeVisible();
  expect(within(dialog).queryByRole("combobox", { name: "Host" })).toBeNull();
  fail = false;
  await user.click(within(dialog).getByRole("button", { name: "Retry" }));

  const host = await within(dialog).findByRole("combobox", { name: "Host" });
  expect(within(host).getByRole("option", { name: "nas" })).toBeInTheDocument();
});

test("deleting_a_rule_confirms_names_it_and_toasts", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");

  await user.click(screen.getByRole("button", { name: "Actions for disk.usage on nas" }));
  await user.click(screen.getByRole("menuitem", { name: "Delete" }));
  const dialog = screen.getByRole("dialog", { name: "Delete the known condition for disk.usage?" });
  expect(
    within(dialog).getByText(/Disk usage on \/srv, when used percent is at most 95 on nas/),
  ).toBeVisible();
  expect(within(dialog).getByText(/Matching incidents notify you again/)).toBeVisible();
  expect(within(dialog).getByText("known backup window")).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Delete known condition" }));

  expect(await screen.findByText("Known condition for disk.usage deleted")).toBeVisible();
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(calls.find((call) => call.route === DELETE)?.init.params?.path).toEqual({ id: BACKUP.id });
  expect(await screen.findByText("Security ssh login")).toBeVisible();
  expect(screen.queryByText(/^Disk usage on/)).toBeNull();
});

test("cancelling_the_delete_keeps_the_rule", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");

  await user.click(screen.getByRole("button", { name: "Actions for disk.usage on nas" }));
  await user.click(screen.getByRole("menuitem", { name: "Delete" }));
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));

  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByText(/^Disk usage on \/srv/)).toBeVisible();
  expect(calls.some((call) => call.route === DELETE)).toBe(false);
});

test("undo_after_deleting_recreates_the_rule_with_its_expiry", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");

  await user.click(screen.getByRole("button", { name: "Actions for disk.usage on nas" }));
  await user.click(screen.getByRole("menuitem", { name: "Delete" }));
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", { name: "Delete known condition" }),
  );
  const toasts = await screen.findByRole("log", { name: "Notifications" });
  await user.click(await within(toasts).findByRole("button", { name: "Undo" }));

  expect(await screen.findByText("Known condition for disk.usage restored")).toBeVisible();
  const created = calls.find((call) => call.route === CREATE)?.init.body as AckRuleInput;
  expect(created).toEqual({
    host: "nas",
    check: "disk.usage",
    target_pattern: "/srv",
    predicates: [{ key: "used_percent", op: "at_most", value: "95" }],
    note: "known backup window",
    expires_in_minutes: null,
    expires_at: BACKUP.expires_at,
  });
});

test("a_failed_delete_stays_in_the_dialog_and_says_why", async () => {
  route(DELETE, () => failure(500, "internal", "The hub could not reach its database."));
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Security ssh login");

  await user.click(screen.getByRole("button", { name: "Actions for disk.usage on nas" }));
  await user.click(screen.getByRole("menuitem", { name: "Delete" }));
  const dialog = screen.getByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: "Delete known condition" }));

  expect(await within(dialog).findByRole("alert")).toHaveTextContent(
    "Could not delete the known condition",
  );
  expect(screen.queryByText("Known condition for disk.usage deleted")).toBeNull();
});

test("a_failed_load_says_so_and_retries", async () => {
  let fail = true;
  route(RULES, () => (fail ? failure(500, "internal", "The hub is busy.") : stored));
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);

  expect(await screen.findByText("Could not load the known conditions")).toBeVisible();
  expect(screen.queryByText("No known conditions yet")).toBeNull();
  fail = false;
  await user.click(screen.getByRole("button", { name: "Retry" }));
  expect(await screen.findByText("Security ssh login")).toBeVisible();
});

test("shows_a_skeleton_while_loading", async () => {
  route(RULES, () => new Promise(() => undefined));
  renderAt("/rules", "/rules", <RulesPage />);

  expect(await screen.findByRole("heading", { name: "Known conditions" })).toBeVisible();
  expect(screen.getByText("Loading…")).toBeInTheDocument();
  expect(screen.queryByRole("table")).toBeNull();
  expect(screen.queryByText("No known conditions yet")).toBeNull();
});

test("an_empty_list_says_why_and_offers_to_add_one", async () => {
  stored = [];
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);

  expect(await screen.findByText("No known conditions yet")).toBeVisible();
  expect(screen.getByText(/Every matching incident notifies you/)).toBeVisible();
  const buttons = screen.getAllByRole("button", { name: "Add known condition" });
  await user.click(buttons[buttons.length - 1] as HTMLElement);
  expect(await screen.findByRole("dialog", { name: "Add a known condition" })).toBeVisible();
});

test("known_conditions_follow_the_selected_host_and_keep_the_ones_for_any_host", async () => {
  stored = [BACKUP, OLD, FOREVER];
  renderAt("/rules?host=nas", "/rules", <RulesPage />);

  expect(await screen.findByText(/^Disk usage on \/srv/)).toBeVisible();
  expect(screen.getByText("Security ssh login"), "a rule for any host applies here").toBeVisible();
  expect(screen.queryByText("Memory available")).toBeNull();
  expect(screen.getByText(/^Showing the known conditions that apply to nas\./)).toBeVisible();
});

test("the_add_dialog_starts_on_the_selected_host", async () => {
  const user = userEvent.setup();
  renderAt("/rules?host=nas", "/rules", <RulesPage />);
  await screen.findByText(/^Disk usage on \/srv/);
  const dialog = await openAdd(user);

  expect(await within(dialog).findByRole("combobox", { name: "Host" })).toHaveValue("nas");
});

test("a_selected_host_with_no_conditions_says_the_others_exist", async () => {
  stored = [FOREVER];
  renderAt("/rules?host=nas", "/rules", <RulesPage />);

  expect(await screen.findByText("No known conditions for nas")).toBeVisible();
  expect(screen.getByText(/Choose All hosts to see them/)).toBeVisible();
});

test("all_hosts_shows_every_rule_without_a_note", async () => {
  stored = [BACKUP, FOREVER];
  renderAt("/rules", "/rules", <RulesPage />);

  expect(await screen.findByText("Memory available")).toBeVisible();
  expect(screen.queryByText(/^Showing the known conditions that apply to/)).toBeNull();
});

test("rules_say_who_created_them_and_when_and_admit_when_that_is_unknown", async () => {
  stored = [
    { ...BACKUP, created_at: "2026-11-01T08:00:00Z", created_by: "admin" },
    { ...OLD, created_at: null, created_by: null },
  ];
  renderAt("/rules", "/rules", <RulesPage />);

  await screen.findByText("Disk usage on /srv, when used percent is at most 95");
  const backup = rowOf("Disk usage on /srv, when used percent is at most 95");
  expect(within(backup).getByText("admin")).toBeVisible();
  expect(within(rowOf("Security ssh login")).getByText("Unknown")).toBeVisible();
});
