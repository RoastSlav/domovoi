import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AckRule, AckRuleInput, Predicate } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { T0, incident } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { SshLoginTable } from "./SshLoginTable";
import { SshLoginsTab } from "./SshLoginsTab";
import { MANY_FAILURES, loginsOf, type LoginEntry } from "./sshLogins";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const NOW = new Date("2026-11-01T12:00:00Z");

beforeEach(resetApi);

afterEach(() => {
  vi.useRealTimers();
});

function posts(path: string) {
  return calls.filter((c) => c.route === `POST ${path}`);
}

/** An expected SSH login as the API returns it, overridable. */
function loginRule(
  id: string,
  user: string,
  predicates: Predicate[],
  expires: string | null,
  overrides: Partial<AckRule> = {},
): AckRule {
  return {
    id,
    host: "nas",
    check: "security.ssh_login",
    target_pattern: user,
    predicates,
    note: "Expected SSH login",
    expires_at: expires,
    ...overrides,
  };
}

/** The login incident the agent raises for `user` logging in from `address`. */
function login(user: string, address: string, network: string, overrides = {}) {
  return incident({
    id: `inc-${user}`,
    check: "security.ssh_login",
    target: user,
    title: `SSH login for ${user} from ${address} (${network})`,
    occurrences: 1,
    ...overrides,
  });
}

/** Serves the history `incidents` and the stored `rules`, which create and delete change. */
function serve(rules: AckRule[] = [], incidents = [login("alice", "192.168.1.20", "lan")]) {
  route("GET /api/v1/incidents", () => ({ items: incidents, next_cursor: null }));
  route("GET /api/v1/ack-rules", () => [...rules]);
  route("POST /api/v1/ack-rules", (init) => {
    const body = init.body as AckRuleInput;
    const created: AckRule = {
      id: "rule-new",
      host: body.host ?? null,
      check: body.check,
      target_pattern: body.target_pattern ?? null,
      predicates: body.predicates,
      note: body.note,
      expires_at:
        body.expires_in_minutes == null
          ? null
          : new Date(NOW.getTime() + body.expires_in_minutes * 60_000).toISOString(),
    };
    rules.push(created);
    return created;
  });
  route("DELETE /api/v1/ack-rules/{id}", (init) => {
    const at = rules.findIndex((rule) => rule.id === init.params?.path?.id);
    rules.splice(at, 1);
    return null;
  });
}

function show() {
  renderAt("/hosts/nas", "/hosts/:name", <SshLoginsTab host="nas" />);
}

/** Starts the page with fake time, so "in 3 days" is stable, and a user that follows it. */
function atNow() {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  return userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
}

function failed(address: string, id: string): LoginEntry {
  return { id, user: "root", address, network: "wan", at: T0, count: 1, result: "failed" };
}

test("the_login_history_lists_user_source_address_time_and_result", async () => {
  serve([], [login("alice", "192.168.1.20", "lan"), login("bob", "203.0.113.9", "wan")]);
  show();
  const table = await screen.findByRole("table", { name: "SSH logins on nas" });
  const alice = within(table).getByText("alice").closest("tr") as HTMLElement;
  expect(within(alice).getByText("192.168.1.20")).toBeVisible();
  expect(within(alice).getByText("Local network")).toBeVisible();
  expect(within(alice).getByText("Accepted")).toBeVisible();
  const bob = within(table).getByText("bob").closest("tr") as HTMLElement;
  expect(within(bob).getByText("Internet")).toBeVisible();
  expect(calls.find((c) => c.route === "GET /api/v1/incidents")?.init.params?.query).toMatchObject({
    host: "nas",
    q: "security.ssh_login",
  });
});

test("the_source_filter_narrows_the_history_to_the_internet_or_the_local_network", async () => {
  serve([], [login("alice", "192.168.1.20", "lan"), login("bob", "203.0.113.9", "wan")]);
  const user = userEvent.setup();
  show();
  await screen.findByRole("table", { name: "SSH logins on nas" });

  await user.click(screen.getByRole("radio", { name: "Internet" }));
  const table = screen.getByRole("table", { name: "SSH logins on nas" });
  expect(within(table).queryByText("alice")).toBeNull();
  expect(within(table).getByText("bob")).toBeVisible();

  await user.click(screen.getByRole("radio", { name: "Local network" }));
  expect(within(table).getByText("alice")).toBeVisible();
  expect(within(table).queryByText("bob")).toBeNull();
});

test("a_host_with_no_logins_says_how_they_get_there", async () => {
  serve([], []);
  show();
  expect(await screen.findByText("No SSH logins recorded on nas")).toBeVisible();
});

test("a_login_history_that_fails_to_load_shows_retry_and_no_empty_state", async () => {
  serve();
  route("GET /api/v1/incidents", () => failure(500, "internal", "boom"));
  show();
  const section = (await screen.findByRole("heading", { name: "Recent logins" })).closest(
    "section",
  ) as HTMLElement;
  expect(await within(section).findByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.queryByText(/No SSH logins recorded/)).toBeNull();
});

test("many_failed_ssh_logins_from_one_address_read_as_danger_and_one_failure_as_a_warning", async () => {
  const user = userEvent.setup();
  const entries = [
    ...Array.from({ length: MANY_FAILURES }, (_, i) => failed("203.0.113.7", `a${i}`)),
    failed("198.51.100.4", "b"),
  ];
  renderAt("/", "/", <SshLoginTable all={entries} shown={entries} host="nas" />);
  const many = screen.getAllByText("Many failures");
  expect(many).toHaveLength(MANY_FAILURES);
  expect(many[0]?.closest("[data-tone]")).toHaveAttribute("data-tone", "danger");
  const one = screen.getByText("Failed");
  expect(one.closest("[data-tone]")).toHaveAttribute("data-tone", "warning");

  await user.hover(many[0] as HTMLElement);
  expect(await screen.findByRole("tooltip")).toHaveTextContent(/guessing passwords/);
});

test("an_accepted_login_reads_neutral_and_a_failure_is_not_inferred_from_the_agent", () => {
  const [entry] = loginsOf([login("alice", "10.0.0.2", "lan")]);
  expect(entry).toMatchObject({
    user: "alice",
    address: "10.0.0.2",
    network: "lan",
    result: "accepted",
  });
});

test("the_expected_logins_list_shows_only_the_ones_that_cover_this_host", async () => {
  atNow();
  serve([
    loginRule(
      "r-1",
      "alice",
      [{ key: "ip", op: "in_network", value: "192.168.1.0/24" }],
      "2026-11-04T12:00:00Z",
    ),
    loginRule("r-2", "bob", [{ key: "network", op: "equals", value: "lan" }], null),
    loginRule("r-3", "carol", [], null, { check: "disk.usage" }),
    loginRule("r-4", "dave", [], null, { host: "pi" }),
    loginRule("r-5", "erin", [], null, { host: null }),
  ]);
  show();
  const table = await screen.findByRole("table", { name: "Expected logins on nas" });
  const rows = within(table).getAllByRole("row").slice(1);
  // A rule for any host silences logins here too, so it is listed and labelled.
  expect(rows.map((row) => row.textContent)).toEqual([
    "alicethe range 192.168.1.0/24in 3 daysRemove",
    "bobthe local networknever" + "Remove",
    "erinanywhere on any hostneverRemove",
  ]);
  expect(within(table).queryByText(/carol|dave/)).toBeNull();
});

test("an_empty_expected_logins_list_says_what_it_means_and_what_to_do", async () => {
  serve([]);
  show();
  expect(await screen.findByText("No logins are marked as expected on nas")).toBeVisible();
});

test("login_window_asks_first_and_expects_local_network_logins_for_an_hour", async () => {
  const user = atNow();
  serve();
  show();

  await user.click(await screen.findByRole("button", { name: "I’m logging in now" }));
  expect(posts("/api/v1/ack-rules")).toHaveLength(0);
  await user.click(screen.getByRole("button", { name: "Open the window" }));

  expect(posts("/api/v1/ack-rules")[0]?.init.body).toEqual({
    host: "nas",
    check: "security.ssh_login",
    target_pattern: null,
    predicates: [{ key: "network", op: "equals", value: "lan" }],
    note: "Login window opened from the web UI",
    expires_in_minutes: 60,
  });
  expect(await screen.findByText("Login window opened on nas for 1 hour")).toBeVisible();
  expect(
    await screen.findByText(/^A login window is open on nas, so logins from the local network/),
  ).toBeVisible();
});

test("cancelling_the_login_window_opens_nothing", async () => {
  const user = userEvent.setup();
  serve();
  show();
  await user.click(await screen.findByRole("button", { name: "I’m logging in now" }));
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(posts("/api/v1/ack-rules")).toHaveLength(0);
  expect(screen.queryByText(/^A login window is open/)).toBeNull();
});

test("login_window_includes_internet_logins_only_when_asked", async () => {
  const user = atNow();
  serve();
  show();
  await user.click(await screen.findByRole("button", { name: "I’m logging in now" }));
  await user.click(screen.getByRole("checkbox", { name: "Include logins from the internet" }));
  await user.click(screen.getByRole("button", { name: "Open the window" }));

  expect(posts("/api/v1/ack-rules")[0]?.init.body).toMatchObject({ predicates: [] });
  expect(await screen.findByText(/including from the internet, is expected/)).toBeVisible();
});

test("the_login_window_note_shows_after_a_reload_and_not_after_it_expires", async () => {
  atNow();
  serve([
    loginRule(
      "r-1",
      "x",
      [{ key: "network", op: "equals", value: "lan" }],
      "2026-11-01T12:30:00Z",
      {
        target_pattern: null,
      },
    ),
    loginRule("r-2", "y", [], "2026-10-31T13:00:00Z", { target_pattern: null }),
  ]);
  show();
  expect(await screen.findByText(/^A login window is open on nas/)).toHaveTextContent(
    "A login window is open on nas, so logins from the local network are expected.",
  );
  expect(screen.getAllByText(/^A login window is open/)).toHaveLength(1);
});

test("an_expected_login_for_the_local_network_is_sent_with_its_user_and_lifetime", async () => {
  const user = userEvent.setup();
  serve();
  show();
  await user.click(await screen.findByRole("button", { name: "Add expected login" }));
  await user.type(screen.getByLabelText("User"), "alice");
  await user.selectOptions(screen.getByLabelText("From"), "lan");
  await user.selectOptions(screen.getByLabelText("For"), "1440");
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", { name: "Add expected login" }),
  );

  expect(posts("/api/v1/ack-rules")[0]?.init.body).toEqual({
    host: "nas",
    check: "security.ssh_login",
    target_pattern: "alice",
    predicates: [{ key: "network", op: "equals", value: "lan" }],
    note: "Expected SSH login",
    expires_in_minutes: 1440,
  });
  expect(await screen.findByText("Expected login added")).toBeVisible();
});

test("an_expected_login_for_one_address_is_sent_as_typed", async () => {
  const user = userEvent.setup();
  serve();
  show();
  await user.click(await screen.findByRole("button", { name: "Add expected login" }));
  await user.type(screen.getByLabelText("User"), "alice");
  await user.selectOptions(screen.getByLabelText("From"), "range");
  await user.type(screen.getByLabelText("Address or range"), "192.168.1.20");
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", { name: "Add expected login" }),
  );
  expect(posts("/api/v1/ack-rules")[0]?.init.body).toMatchObject({
    target_pattern: "alice",
    predicates: [{ key: "ip", op: "in_network", value: "192.168.1.20" }],
    expires_in_minutes: 60,
  });
});

test("the_address_is_needed_before_a_login_is_expected_and_the_field_says_so", async () => {
  const user = userEvent.setup();
  serve();
  show();
  await user.click(await screen.findByRole("button", { name: "Add expected login" }));
  await user.selectOptions(screen.getByLabelText("From"), "range");
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", { name: "Add expected login" }),
  );

  const field = screen.getByLabelText("Address or range");
  expect(field).toBeInvalid();
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Type the address, or the range in CIDR form, for example 192.168.1.0/24.",
  );
  expect(posts("/api/v1/ack-rules")).toHaveLength(0);
});

test("removing_an_expected_login_asks_then_deletes_its_rule", async () => {
  const user = userEvent.setup();
  serve([
    loginRule("r-1", "alice", [{ key: "ip", op: "in_network", value: "192.168.1.0/24" }], null),
  ]);
  show();
  await user.click(
    await screen.findByRole("button", {
      name: "Remove the expected login alice from the range 192.168.1.0/24",
    }),
  );
  expect(calls.filter((c) => c.route === "DELETE /api/v1/ack-rules/{id}")).toHaveLength(0);

  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Remove" }));

  expect(calls.find((c) => c.route === "DELETE /api/v1/ack-rules/{id}")?.init.params?.path).toEqual(
    { id: "r-1" },
  );
  expect(await screen.findByText("No logins are marked as expected on nas")).toBeVisible();
});

test("cancelling_the_removal_keeps_the_expected_login", async () => {
  const user = userEvent.setup();
  serve([loginRule("r-1", "alice", [], null)]);
  show();
  await user.click(
    await screen.findByRole("button", { name: "Remove the expected login alice from anywhere" }),
  );
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(calls.filter((c) => c.route === "DELETE /api/v1/ack-rules/{id}")).toHaveLength(0);
  expect(screen.getByRole("table", { name: "Expected logins on nas" })).toHaveTextContent("alice");
});
